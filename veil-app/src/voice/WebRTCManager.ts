import { wsSend } from '../api';
import type { VoiceOfferEvent } from '../types/voice';

/**
 * Singleton that manages the WebRTC peer connection to the SFU.
 *
 * Lifecycle: join → receive SDP offer → create answer → exchange ICE → media flows.
 * No E2E encryption — the server handles media routing directly.
 */
class WebRTCManagerSingleton {
  private pc: RTCPeerConnection | null = null;
  private localStream: MediaStream | null = null;

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
   * Handle an SDP offer from the SFU. On the first call, creates the peer
   * connection. On subsequent calls (renegotiation), reuses the existing one.
   */
  async handleOffer(offer: VoiceOfferEvent): Promise<void> {
    await this.acquireMicrophone();

    // Renegotiation: PeerConnection already exists — just update SDP.
    if (this.pc) {
      await this.pc.setRemoteDescription({
        type: 'offer',
        sdp: offer.sdp,
      });
      const answer = await this.pc.createAnswer();
      await this.pc.setLocalDescription(answer);
      wsSend('voice_sdp_answer', { sdp: answer.sdp });
      return;
    }

    // Initial connection: create new PeerConnection.
    this.pc = new RTCPeerConnection({
      iceServers: [{ urls: 'stun:stun.l.google.com:19302' }],
    });

    // ICE candidate handler — send via WebSocket
    this.pc.onicecandidate = (event) => {
      if (event.candidate) {
        wsSend('voice_ice_candidate', {
          candidate: JSON.stringify(event.candidate.toJSON()),
        });
      }
    };

    // Track handler — remote audio from other participants
    this.pc.ontrack = (event) => {
      const stream = event.streams[0] ?? new MediaStream([event.track]);
      this.onTrackAdded?.('remote', event.track, stream);
    };

    // Add local audio track
    if (this.localStream) {
      for (const track of this.localStream.getAudioTracks()) {
        this.pc.addTrack(track, this.localStream);
      }
    }

    // Set remote offer and create answer
    await this.pc.setRemoteDescription({
      type: 'offer',
      sdp: offer.sdp,
    });

    const answer = await this.pc.createAnswer();
    await this.pc.setLocalDescription(answer);

    // Send answer to SFU via WebSocket
    wsSend('voice_sdp_answer', { sdp: answer.sdp });
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
    if (this.localStream) {
      for (const track of this.localStream.getTracks()) {
        track.stop();
      }
      this.localStream = null;
    }

    if (this.pc) {
      this.pc.close();
      this.pc = null;
    }
  }
}

/** Global singleton */
export const webRTCManager = new WebRTCManagerSingleton();
