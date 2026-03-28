import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store/appStore';
import type { ChatMessage } from '../types/messages';
import type {
  VoiceOfferEvent,
  VoiceIceCandidateEvent,
  VoiceParticipantEvent,
  VoiceSpeakingEvent,
} from '../types/voice';
import { webRTCManager } from '../voice/WebRTCManager';

/**
 * Subscribes to Tauri backend events and updates the Zustand store.
 * Call once in the app after identity is confirmed.
 */
export function useTauriEvents() {
  useEffect(() => {
    const unlisten: Array<() => void> = [];

    // Message received from a peer (P2P or relay)
    listen<{
      id: string;
      senderId: string;
      senderName: string;
      content?: string;
      timestamp: number;
      isSelf: boolean;
      groupId: string;
      channelId: string;
      replyToId?: string;
      kindType?: string;
      blobId?: string;
      width?: number;
      height?: number;
      thumbnailB64?: string;
      filename?: string;
      sizeBytes?: number;
      durationSecs?: number;
      waveform?: number[];
    }>('veil://message-received', (event) => {
      const m = event.payload;
      const state = useAppStore.getState();
      if (m.groupId !== state.activeGroupId) {
        useAppStore.setState({
          groups: state.groups.map((g) =>
            g.id === m.groupId ? { ...g, unreadCount: g.unreadCount + 1 } : g,
          ),
        });
        return;
      }
      if (state.messages.some((msg) => msg.id === m.id)) return;

      let kind: ChatMessage['kind'];
      switch (m.kindType) {
        case 'image':
          kind = {
            type: 'image',
            blobId: m.blobId!,
            width: m.width ?? 0,
            height: m.height ?? 0,
            thumbnailUrl: m.thumbnailB64 ? `data:image/jpeg;base64,${m.thumbnailB64}` : undefined,
          };
          break;
        case 'video':
          kind = {
            type: 'video',
            blobId: m.blobId!,
            durationSecs: m.durationSecs ?? 0,
            thumbnailUrl: m.thumbnailB64 ? `data:image/jpeg;base64,${m.thumbnailB64}` : undefined,
          };
          break;
        case 'audio':
          kind = {
            type: 'audio',
            blobId: m.blobId!,
            durationSecs: m.durationSecs ?? 0,
            waveform: m.waveform ?? [],
          };
          break;
        case 'file':
          kind = {
            type: 'file',
            blobId: m.blobId!,
            filename: m.filename ?? 'file',
            sizeBytes: m.sizeBytes ?? 0,
          };
          break;
        default:
          kind = { type: 'text', content: m.content ?? '' };
      }

      const msg: ChatMessage = {
        id: m.id,
        senderId: m.senderId,
        senderName: m.senderName,
        senderRole: 'member',
        kind,
        timestamp: m.timestamp * 1000,
        edited: false,
        pinned: false,
        ephemeral: false,
        expiresAt: null,
        replyTo: null,
        reactions: [],
        isSelf: m.isSelf,
      };

      useAppStore.setState({ messages: [...state.messages, msg] });
    }).then((fn) => unlisten.push(fn));

    // Peer connected
    listen<{ peerId: string; connId: number }>('veil://peer-connected', () => {
      const state = useAppStore.getState();
      useAppStore.setState({
        connection: {
          ...state.connection,
          state: 'connected',
          peerCount: state.connection.peerCount + 1,
        },
      });
    }).then((fn) => unlisten.push(fn));

    // Peer disconnected
    listen<{ connId: number }>('veil://peer-disconnected', () => {
      const state = useAppStore.getState();
      useAppStore.setState({
        connection: {
          ...state.connection,
          peerCount: Math.max(0, state.connection.peerCount - 1),
        },
      });
    }).then((fn) => unlisten.push(fn));

    // Relay connected
    listen('veil://relay-connected', () => {
      const state = useAppStore.getState();
      useAppStore.setState({
        connection: { ...state.connection, relayConnected: true },
      });
    }).then((fn) => unlisten.push(fn));

    // Relay disconnected
    listen('veil://relay-disconnected', () => {
      const state = useAppStore.getState();
      useAppStore.setState({
        connection: { ...state.connection, relayConnected: false },
      });
    }).then((fn) => unlisten.push(fn));

    // Network ready
    listen<{ localAddr: string }>('veil://network-ready', () => {
      const state = useAppStore.getState();
      useAppStore.setState({
        connection: { ...state.connection, state: 'connected' },
      });
    }).then((fn) => unlisten.push(fn));

    // Connection failed
    listen<string>('veil://connection-failed', () => {
      const state = useAppStore.getState();
      useAppStore.setState({
        connection: { ...state.connection, state: 'failed' },
      });
    }).then((fn) => unlisten.push(fn));

    // ── Voice events ──

    // SDP offer from SFU — establish WebRTC connection
    listen<VoiceOfferEvent>('veil://voice-offer', async (event) => {
      const offer = event.payload;
      const state = useAppStore.getState();
      useAppStore.setState({
        voice: {
          ...state.voice,
          roomId: offer.roomId,
          participants: offer.participants.map((peerId) => ({
            peerId,
            displayName: peerId.substring(0, 8),
            isMuted: false,
            isSpeaking: false,
          })),
        },
      });
      try {
        await webRTCManager.handleOffer(offer);
      } catch (e) {
        console.error('Failed to handle voice offer:', e);
      }
    }).then((fn) => unlisten.push(fn));

    // ICE candidate from SFU
    listen<VoiceIceCandidateEvent>('veil://voice-ice-candidate', async (event) => {
      try {
        await webRTCManager.handleIceCandidate(event.payload.candidate);
      } catch (e) {
        console.error('Failed to handle ICE candidate:', e);
      }
    }).then((fn) => unlisten.push(fn));

    // Participant joined voice room
    listen<VoiceParticipantEvent>('veil://voice-participant-joined', (event) => {
      const { peerId } = event.payload;
      const state = useAppStore.getState();
      if (state.voice.participants.some((p) => p.peerId === peerId)) return;
      useAppStore.setState({
        voice: {
          ...state.voice,
          participants: [
            ...state.voice.participants,
            { peerId, displayName: peerId.substring(0, 8), isMuted: false, isSpeaking: false },
          ],
        },
      });
    }).then((fn) => unlisten.push(fn));

    // Participant left voice room
    listen<VoiceParticipantEvent>('veil://voice-participant-left', (event) => {
      const { peerId } = event.payload;
      const state = useAppStore.getState();
      useAppStore.setState({
        voice: {
          ...state.voice,
          participants: state.voice.participants.filter((p) => p.peerId !== peerId),
        },
      });
    }).then((fn) => unlisten.push(fn));

    // Speaking indicator
    listen<VoiceSpeakingEvent>('veil://voice-speaking', (event) => {
      useAppStore.getState().updateSpeaking(event.payload.peerId, event.payload.speaking);
    }).then((fn) => unlisten.push(fn));

    // Key rotation — re-derive voice encryption key
    listen('veil://voice-key-rotated', async () => {
      if (useAppStore.getState().voice.inRoom) {
        try {
          await webRTCManager.rotateKey();
        } catch (e) {
          console.error('Failed to rotate voice key:', e);
        }
      }
    }).then((fn) => unlisten.push(fn));

    return () => {
      unlisten.forEach((fn) => fn());
    };
  }, []);
}
