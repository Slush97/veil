import { invoke } from '@tauri-apps/api/core';
import type { VoiceOfferEvent } from '../types/voice';

/**
 * Singleton that manages the WebRTC peer connection to the SFU.
 *
 * Lifecycle: join → receive SDP offer → create answer → exchange ICE → media flows.
 * Encryption is handled by RTCRtpScriptTransform (see encryptionWorker.ts).
 */
class WebRTCManagerSingleton {
  private pc: RTCPeerConnection | null = null;
  private localStream: MediaStream | null = null;
  private encryptionKey: CryptoKey | null = null;
  private keyGeneration: number = 0;

  /** Active sender transform workers */
  private senderWorker: Worker | null = null;
  private receiverWorkers: Map<string, Worker> = new Map();

  /** Callbacks for UI updates */
  onTrackAdded?: (peerId: string, track: MediaStreamTrack, stream: MediaStream) => void;
  onTrackRemoved?: (peerId: string) => void;

  get isConnected(): boolean {
    return this.pc !== null && this.pc.connectionState === 'connected';
  }

  get isMuted(): boolean {
    if (!this.localStream) return true;
    const audioTrack = this.localStream.getAudioTracks()[0];
    return audioTrack ? !audioTrack.enabled : true;
  }

  /**
   * Acquire microphone and prepare for connection.
   */
  async acquireMicrophone(): Promise<void> {
    if (this.localStream) return;
    this.localStream = await navigator.mediaDevices.getUserMedia({
      audio: {
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true,
      },
      video: false,
    });
  }

  /**
   * Load the voice encryption key from the backend.
   */
  async loadEncryptionKey(): Promise<void> {
    const [hexKey, generation] = await invoke<[string, number]>('get_voice_encryption_key');
    const keyBytes = hexToBytes(hexKey);
    this.encryptionKey = await crypto.subtle.importKey(
      'raw',
      keyBytes,
      { name: 'AES-GCM' },
      false,
      ['encrypt', 'decrypt'],
    );
    this.keyGeneration = generation;

    // Notify encryption workers of the new key
    this.broadcastKeyToWorkers(hexKey, generation);
  }

  /**
   * Handle an SDP offer from the SFU. On the first call, creates the peer
   * connection. On subsequent calls (renegotiation), reuses the existing one.
   */
  async handleOffer(offer: VoiceOfferEvent): Promise<void> {
    await this.acquireMicrophone();
    await this.loadEncryptionKey();

    // Renegotiation: PeerConnection already exists — just update SDP.
    if (this.pc) {
      await this.pc.setRemoteDescription({
        type: 'offer',
        sdp: offer.sdp,
      });
      const answer = await this.pc.createAnswer();
      await this.pc.setLocalDescription(answer);
      await invoke('voice_sdp_answer', { sdp: answer.sdp });
      return;
    }

    // Initial connection: create new PeerConnection.
    this.pc = new RTCPeerConnection({
      iceServers: [{ urls: 'stun:stun.l.google.com:19302' }],
      // @ts-expect-error - encodedInsertableStreams is not in the type defs yet
      encodedInsertableStreams: true,
    });

    // ICE candidate handler
    this.pc.onicecandidate = async (event) => {
      if (event.candidate) {
        try {
          await invoke('voice_ice_candidate', {
            candidate: JSON.stringify(event.candidate.toJSON()),
          });
        } catch (e) {
          console.error('Failed to send ICE candidate:', e);
        }
      }
    };

    // Track handler — remote audio from other participants
    this.pc.ontrack = (event) => {
      const stream = event.streams[0] ?? new MediaStream([event.track]);
      // Set up decryption transform on the receiver
      this.setupReceiverTransform(event.receiver, event.track.id);
      this.onTrackAdded?.('remote', event.track, stream);
    };

    // Add local audio track
    if (this.localStream) {
      for (const track of this.localStream.getAudioTracks()) {
        const sender = this.pc.addTrack(track, this.localStream);
        this.setupSenderTransform(sender);
      }
    }

    // Set remote offer and create answer
    await this.pc.setRemoteDescription({
      type: 'offer',
      sdp: offer.sdp,
    });

    const answer = await this.pc.createAnswer();
    await this.pc.setLocalDescription(answer);

    // Send answer to SFU via Tauri backend
    await invoke('voice_sdp_answer', { sdp: answer.sdp });
  }

  /**
   * Handle a trickle ICE candidate from the SFU.
   */
  async handleIceCandidate(candidateJson: string): Promise<void> {
    if (!this.pc) return;
    const candidate = new RTCIceCandidate(JSON.parse(candidateJson));
    await this.pc.addIceCandidate(candidate);
  }

  /**
   * Set local mute state.
   */
  setMuted(muted: boolean): void {
    if (!this.localStream) return;
    for (const track of this.localStream.getAudioTracks()) {
      track.enabled = !muted;
    }
  }

  /**
   * Disconnect from the voice channel and clean up all resources.
   */
  disconnect(): void {
    // Stop local tracks
    if (this.localStream) {
      for (const track of this.localStream.getTracks()) {
        track.stop();
      }
      this.localStream = null;
    }

    // Close peer connection
    if (this.pc) {
      this.pc.close();
      this.pc = null;
    }

    // Terminate workers
    this.senderWorker?.terminate();
    this.senderWorker = null;
    for (const worker of this.receiverWorkers.values()) {
      worker.terminate();
    }
    this.receiverWorkers.clear();

    this.encryptionKey = null;
  }

  /**
   * Set up an encryption transform on an RTP sender using RTCRtpScriptTransform.
   */
  private setupSenderTransform(sender: RTCRtpSender): void {
    if (!('transform' in sender)) return;

    const worker = new Worker(
      new URL('./encryptionWorker.ts', import.meta.url),
      { type: 'module' },
    );
    worker.postMessage({
      type: 'init',
      direction: 'encrypt',
      keyHex: this.encryptionKey ? '' : '', // Key sent separately
      generation: this.keyGeneration,
    });
    // Send the actual key material
    if (this.encryptionKey) {
      crypto.subtle.exportKey('raw', this.encryptionKey).then((raw) => {
        worker.postMessage({
          type: 'setKey',
          keyBytes: new Uint8Array(raw),
          generation: this.keyGeneration,
        });
      });
    }

    (sender as any).transform = new (globalThis as any).RTCRtpScriptTransform(worker, {
      direction: 'encrypt',
    });
    this.senderWorker = worker;
  }

  /**
   * Set up a decryption transform on an RTP receiver.
   */
  private setupReceiverTransform(receiver: RTCRtpReceiver, trackId: string): void {
    if (!('transform' in receiver)) return;

    const worker = new Worker(
      new URL('./encryptionWorker.ts', import.meta.url),
      { type: 'module' },
    );
    worker.postMessage({
      type: 'init',
      direction: 'decrypt',
      generation: this.keyGeneration,
    });
    if (this.encryptionKey) {
      crypto.subtle.exportKey('raw', this.encryptionKey).then((raw) => {
        worker.postMessage({
          type: 'setKey',
          keyBytes: new Uint8Array(raw),
          generation: this.keyGeneration,
        });
      });
    }

    (receiver as any).transform = new (globalThis as any).RTCRtpScriptTransform(worker, {
      direction: 'decrypt',
    });
    this.receiverWorkers.set(trackId, worker);
  }

  /**
   * Broadcast a new key to all active encryption workers (for key rotation).
   */
  private broadcastKeyToWorkers(hexKey: string, generation: number): void {
    const keyBytes = hexToBytes(hexKey);
    const msg = { type: 'setKey' as const, keyBytes, generation };

    this.senderWorker?.postMessage(msg);
    for (const worker of this.receiverWorkers.values()) {
      worker.postMessage(msg);
    }
  }

  /**
   * Called when the group key is rotated — re-derive and distribute.
   */
  async rotateKey(): Promise<void> {
    await this.loadEncryptionKey();
  }
}

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substring(i, i + 2), 16);
  }
  return bytes;
}

/** Global singleton */
export const webRTCManager = new WebRTCManagerSingleton();
