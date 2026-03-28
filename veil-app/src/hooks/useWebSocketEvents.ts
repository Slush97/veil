import { useEffect } from 'react';
import { wsOn, wsOff, onWsStateChange } from '../api';
import { useAppStore } from '../store/appStore';
import type { ChatMessage } from '../types/messages';
import { webRTCManager } from '../voice/WebRTCManager';
import { serverMessageToChat } from '../store/messageMapping';

export function useWebSocketEvents() {
  useEffect(() => {
    // ── new_message ──
    const onNewMessage = (data: any) => {
      const state = useAppStore.getState();
      const msg = serverMessageToChat(data, state.auth.user?.id ?? '');

      // Dedup by id
      if (state.messages.some((m) => m.id === msg.id)) return;

      // If message is for a different server's channel, bump unread
      if (data.channel_id !== state.activeChannelId) {
        // Find which group this channel belongs to - for now just ignore non-active channel messages
        return;
      }

      useAppStore.setState({ messages: [...state.messages, msg] });
    };

    // ── message_edited ──
    const onMessageEdited = (data: any) => {
      const state = useAppStore.getState();
      useAppStore.setState({
        messages: state.messages.map((m) =>
          m.id === data.id
            ? { ...m, kind: { type: 'text' as const, content: data.content ?? '' }, edited: true }
            : m,
        ),
      });
    };

    // ── message_deleted ──
    const onMessageDeleted = (data: any) => {
      const state = useAppStore.getState();
      useAppStore.setState({
        messages: state.messages.filter((m) => m.id !== data.id),
      });
    };

    // ── typing ──
    const onTypingStart = (data: any) => {
      const state = useAppStore.getState();
      if (data.channel_id !== state.activeChannelId) return;
      const userId = data.user_id;
      if (userId === state.auth.user?.id) return;
      useAppStore.setState({
        members: state.members.map((m) =>
          m.peerId === userId ? { ...m, isTyping: true } : m,
        ),
      });
    };

    const onTypingStop = (data: any) => {
      const state = useAppStore.getState();
      const userId = data.user_id;
      useAppStore.setState({
        members: state.members.map((m) =>
          m.peerId === userId ? { ...m, isTyping: false } : m,
        ),
      });
    };

    // ── presence ──
    const onPresenceUpdate = (data: any) => {
      const state = useAppStore.getState();
      useAppStore.setState({
        members: state.members.map((m) =>
          m.peerId === data.user_id ? { ...m, status: data.status } : m,
        ),
      });
    };

    // ── member joined/left ──
    const onMemberJoined = (data: any) => {
      const state = useAppStore.getState();
      if (state.members.some((m) => m.peerId === data.user_id)) return;
      useAppStore.setState({
        members: [
          ...state.members,
          {
            peerId: data.user_id,
            displayName: data.display_name ?? data.username ?? 'Unknown',
            role: data.role ?? 'member',
            status: 'online' as const,
            bio: '',
            statusText: '',
            isTyping: false,
            isSelf: data.user_id === state.auth.user?.id,
          },
        ],
      });
    };

    const onMemberLeft = (data: any) => {
      const state = useAppStore.getState();
      useAppStore.setState({
        members: state.members.filter((m) => m.peerId !== data.user_id),
      });
    };

    // ── voice events ──
    const onVoiceOffer = async (data: any) => {
      const state = useAppStore.getState();
      useAppStore.setState({
        voice: {
          ...state.voice,
          roomId: data.room_id,
          participants: (data.participants ?? []).map((peerId: string) => ({
            peerId,
            displayName: peerId.substring(0, 8),
            isMuted: false,
            isSpeaking: false,
          })),
        },
      });
      try {
        await webRTCManager.handleOffer({
          roomId: data.room_id,
          participantId: data.participant_id,
          sdp: data.sdp,
          voiceEndpoint: data.voice_endpoint,
          participants: data.participants ?? [],
        });
      } catch (e) {
        console.error('Failed to handle voice offer:', e);
      }
    };

    const onVoiceIceCandidate = async (data: any) => {
      try {
        await webRTCManager.handleIceCandidate(data.candidate);
      } catch (e) {
        console.error('Failed to handle ICE candidate:', e);
      }
    };

    const onVoiceParticipantJoined = (data: any) => {
      const peerId = data.peer_id;
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
    };

    const onVoiceParticipantLeft = (data: any) => {
      const peerId = data.peer_id;
      const state = useAppStore.getState();
      useAppStore.setState({
        voice: {
          ...state.voice,
          participants: state.voice.participants.filter((p) => p.peerId !== peerId),
        },
      });
    };

    const onVoiceSpeaking = (data: any) => {
      useAppStore.getState().updateSpeaking(data.peer_id, data.speaking);
    };

    // Register all handlers
    wsOn('new_message', onNewMessage);
    wsOn('message_edited', onMessageEdited);
    wsOn('message_deleted', onMessageDeleted);
    wsOn('typing_start', onTypingStart);
    wsOn('typing_stop', onTypingStop);
    wsOn('presence_update', onPresenceUpdate);
    wsOn('member_joined', onMemberJoined);
    wsOn('member_left', onMemberLeft);
    wsOn('voice_offer', onVoiceOffer);
    wsOn('voice_ice_candidate', onVoiceIceCandidate);
    wsOn('voice_participant_joined', onVoiceParticipantJoined);
    wsOn('voice_participant_left', onVoiceParticipantLeft);
    wsOn('voice_speaking', onVoiceSpeaking);

    // Track WS connection state
    const unsubState = onWsStateChange((wsState) => {
      useAppStore.setState({
        connection: { wsState },
      });
    });

    return () => {
      wsOff('new_message', onNewMessage);
      wsOff('message_edited', onMessageEdited);
      wsOff('message_deleted', onMessageDeleted);
      wsOff('typing_start', onTypingStart);
      wsOff('typing_stop', onTypingStop);
      wsOff('presence_update', onPresenceUpdate);
      wsOff('member_joined', onMemberJoined);
      wsOff('member_left', onMemberLeft);
      wsOff('voice_offer', onVoiceOffer);
      wsOff('voice_ice_candidate', onVoiceIceCandidate);
      wsOff('voice_participant_joined', onVoiceParticipantJoined);
      wsOff('voice_participant_left', onVoiceParticipantLeft);
      wsOff('voice_speaking', onVoiceSpeaking);
      unsubState();
    };
  }, []);
}
