import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { Screen, ConnectionState } from '../types/identity';
import type { Group, Channel, Category, Member } from '../types/groups';
import type { ChatMessage } from '../types/messages';

interface AppState {
  // Identity
  identity: {
    masterPeerId: string | null;
    username: string | null;
    displayName: string;
    bio: string;
    status: string;
    isSetUp: boolean;
  };

  // Recovery phrase (transient, cleared after confirmation)
  recoveryPhrase: string | null;

  // Boot state
  hasExistingIdentity: boolean;

  // Groups
  groups: Group[];
  activeGroupId: string | null;

  // Channels
  channels: Channel[];
  categories: Category[];
  activeChannelId: string | null;

  // Members
  members: Member[];

  // Messages
  messages: ChatMessage[];

  // Connection
  connection: {
    state: ConnectionState;
    relayConnected: boolean;
    peerCount: number;
  };

  // UI
  ui: {
    screen: Screen;
    memberListOpen: boolean;
    searchActive: boolean;
    searchQuery: string;
    showPins: boolean;
    replyingTo: ChatMessage | null;
    editingMessage: ChatMessage | null;
    theme: 'dark' | 'light';
  };

  // Actions
  setScreen: (screen: Screen) => void;
  setIdentity: (identity: AppState['identity']) => void;
  setActiveGroup: (groupId: string | null) => void;
  setActiveChannel: (channelId: string | null) => void;
  toggleMemberList: () => void;
  toggleSearch: () => void;
  setSearchQuery: (query: string) => void;
  togglePins: () => void;
  setReplyingTo: (msg: ChatMessage | null) => void;
  setEditingMessage: (msg: ChatMessage | null) => void;
  addMessage: (msg: ChatMessage) => void;
  setTheme: (theme: 'dark' | 'light') => void;

  // Async actions
  hydrateFromBackend: () => Promise<void>;
  sendMessage: (text: string, replyToId?: string) => Promise<void>;
  switchGroup: (groupId: string) => Promise<void>;
  switchChannel: (channelName: string) => Promise<void>;
}

// ── Store ──

export const useAppStore = create<AppState>((set, get) => ({
  identity: {
    masterPeerId: null,
    username: null,
    displayName: '',
    bio: '',
    status: '',
    isSetUp: false,
  },

  recoveryPhrase: null,
  hasExistingIdentity: false,

  groups: [],
  activeGroupId: null,

  channels: [],
  categories: [
    { id: 'cat-text', name: 'Text Channels', position: 0, collapsed: false },
  ],
  activeChannelId: null,

  members: [],
  messages: [],

  connection: {
    state: 'disconnected',
    relayConnected: false,
    peerCount: 0,
  },

  ui: {
    screen: 'loading',
    memberListOpen: true,
    searchActive: false,
    searchQuery: '',
    showPins: false,
    replyingTo: null,
    editingMessage: null,
    theme: 'dark',
  },

  setScreen: (screen) => set((s) => ({ ui: { ...s.ui, screen } })),
  setIdentity: (identity) => set({ identity }),
  setActiveGroup: (groupId) => set({ activeGroupId: groupId }),
  setActiveChannel: (channelId) => set({ activeChannelId: channelId }),
  toggleMemberList: () => set((s) => ({ ui: { ...s.ui, memberListOpen: !s.ui.memberListOpen } })),
  toggleSearch: () => set((s) => ({ ui: { ...s.ui, searchActive: !s.ui.searchActive } })),
  setSearchQuery: (query) => set((s) => ({ ui: { ...s.ui, searchQuery: query } })),
  togglePins: () => set((s) => ({ ui: { ...s.ui, showPins: !s.ui.showPins } })),
  setReplyingTo: (msg) => set((s) => ({ ui: { ...s.ui, replyingTo: msg } })),
  setEditingMessage: (msg) => set((s) => ({ ui: { ...s.ui, editingMessage: msg } })),
  addMessage: (msg) => set((s) => ({ messages: [...s.messages, msg] })),
  setTheme: (theme) => set((s) => ({ ui: { ...s.ui, theme } })),

  // ── Async actions that call Tauri backend ──

  hydrateFromBackend: async () => {
    try {
      // Load groups
      const groups = await invoke<Array<{ id: string; name: string; memberCount: number; unreadCount: number }>>('get_groups');
      const mappedGroups: Group[] = groups.map((g) => ({
        id: g.id,
        name: g.name,
        description: '',
        unreadCount: g.unreadCount,
      }));

      // Auto-select first group
      const activeGroupId = mappedGroups.length > 0 ? mappedGroups[0].id : null;
      if (activeGroupId) {
        await invoke('set_active_group', { index: 0 });
      }

      // Load channels
      const channels = await invoke<Array<{ name: string; isActive: boolean }>>('get_channels');
      const mappedChannels: Channel[] = channels.map((c, i) => ({
        id: `ch-${c.name}`,
        name: c.name,
        kind: 'text' as const,
        categoryId: 'cat-text',
        position: i,
        unread: false,
      }));
      const activeChannelId = mappedChannels.length > 0 ? mappedChannels[0].id : null;

      if (channels.length > 0) {
        await invoke('set_active_channel', { name: channels[0].name });
      }

      // Load messages
      const msgs = await invoke<Array<{
        id: string;
        senderId: string;
        senderName: string;
        content: string;
        timestamp: number;
        isSelf: boolean;
        channelId: string | null;
        replyToSender: string | null;
        replyToPreview: string | null;
      }>>('get_messages', { limit: 200 });

      const mappedMessages: ChatMessage[] = msgs.map((m) => ({
        id: m.id,
        senderId: m.senderId,
        senderName: m.senderName,
        senderRole: m.isSelf ? 'owner' : 'member',
        kind: { type: 'text' as const, content: m.content },
        timestamp: m.timestamp * 1000, // backend sends unix seconds
        edited: false,
        pinned: false,
        ephemeral: false,
        expiresAt: null,
        replyTo: m.replyToSender
          ? { id: '', senderName: m.replyToSender, preview: m.replyToPreview ?? '' }
          : null,
        reactions: [],
        isSelf: m.isSelf,
      }));

      // Load connection info
      const conn = await invoke<{
        state: string;
        relayConnected: boolean;
        peerCount: number;
        localAddr: string | null;
      }>('get_connection_info');

      set({
        groups: mappedGroups,
        activeGroupId,
        channels: mappedChannels,
        activeChannelId,
        messages: mappedMessages,
        members: [], // will be populated by peer events
        connection: {
          state: (conn.state as ConnectionState) || 'disconnected',
          relayConnected: conn.relayConnected,
          peerCount: conn.peerCount,
        },
      });
    } catch (e) {
      console.error('Failed to hydrate from backend:', e);
    }
  },

  sendMessage: async (text, replyToId) => {
    try {
      const result = await invoke<{
        id: string;
        senderId: string;
        senderName: string;
        content: string;
        timestamp: number;
        isSelf: boolean;
        channelId: string | null;
      }>('send_message', { text, replyToId: replyToId ?? null });

      const msg: ChatMessage = {
        id: result.id,
        senderId: result.senderId,
        senderName: result.senderName,
        senderRole: 'owner',
        kind: { type: 'text', content: result.content },
        timestamp: result.timestamp * 1000,
        edited: false,
        pinned: false,
        ephemeral: false,
        expiresAt: null,
        replyTo: null,
        reactions: [],
        isSelf: true,
      };

      set((s) => ({
        messages: [...s.messages, msg],
        ui: { ...s.ui, replyingTo: null },
      }));
    } catch (e) {
      console.error('Failed to send message:', e);
    }
  },

  switchGroup: async (groupId) => {
    const groups = get().groups;
    const index = groups.findIndex((g) => g.id === groupId);
    if (index === -1) return;

    try {
      await invoke('set_active_group', { index });
      set({ activeGroupId: groupId, messages: [] });

      // Reload channels and messages for the new group
      const channels = await invoke<Array<{ name: string; isActive: boolean }>>('get_channels');
      const mappedChannels: Channel[] = channels.map((c, i) => ({
        id: `ch-${c.name}`,
        name: c.name,
        kind: 'text' as const,
        categoryId: 'cat-text',
        position: i,
        unread: false,
      }));

      if (channels.length > 0) {
        await invoke('set_active_channel', { name: channels[0].name });
      }

      const msgs = await invoke<Array<{
        id: string; senderId: string; senderName: string; content: string;
        timestamp: number; isSelf: boolean; channelId: string | null;
        replyToSender: string | null; replyToPreview: string | null;
      }>>('get_messages', { limit: 200 });

      const mappedMessages: ChatMessage[] = msgs.map((m) => ({
        id: m.id,
        senderId: m.senderId,
        senderName: m.senderName,
        senderRole: m.isSelf ? 'owner' : 'member',
        kind: { type: 'text' as const, content: m.content },
        timestamp: m.timestamp * 1000,
        edited: false, pinned: false, ephemeral: false, expiresAt: null,
        replyTo: m.replyToSender
          ? { id: '', senderName: m.replyToSender, preview: m.replyToPreview ?? '' }
          : null,
        reactions: [],
        isSelf: m.isSelf,
      }));

      set({
        channels: mappedChannels,
        activeChannelId: mappedChannels.length > 0 ? mappedChannels[0].id : null,
        messages: mappedMessages,
      });
    } catch (e) {
      console.error('Failed to switch group:', e);
    }
  },

  switchChannel: async (channelName) => {
    try {
      await invoke('set_active_channel', { name: channelName });
      set({ activeChannelId: `ch-${channelName}` });
      // Messages are per-group in the current backend, but we track the channel
      // for future per-channel filtering
    } catch (e) {
      console.error('Failed to switch channel:', e);
    }
  },
}));
