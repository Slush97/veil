import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useAppStore } from '../store/appStore';
import type { ChatMessage } from '../types/messages';

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
      content: string;
      timestamp: number;
      isSelf: boolean;
      groupId: string;
      channelId: string;
      replyToId?: string;
    }>('veil://message-received', (event) => {
      const m = event.payload;
      // Only add if it's for the active group and not a duplicate
      const state = useAppStore.getState();
      if (m.groupId !== state.activeGroupId) {
        // Increment unread for that group
        useAppStore.setState({
          groups: state.groups.map((g) =>
            g.id === m.groupId ? { ...g, unreadCount: g.unreadCount + 1 } : g,
          ),
        });
        return;
      }
      // Check for duplicate
      if (state.messages.some((msg) => msg.id === m.id)) return;

      const msg: ChatMessage = {
        id: m.id,
        senderId: m.senderId,
        senderName: m.senderName,
        senderRole: 'member',
        kind: { type: 'text', content: m.content },
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

    return () => {
      unlisten.forEach((fn) => fn());
    };
  }, []);
}
