import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { Screen, ConnectionState } from '../types/identity';
import type { Group, Channel, Category, Member } from '../types/groups';
import type { ChatMessage } from '../types/messages';
import type { VoiceParticipant, VoiceState } from '../types/voice';
import { webRTCManager } from '../voice/WebRTCManager';

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

  // Voice
  voice: VoiceState;

  // Relay hosting
  relayHosting: {
    active: boolean;
    addr: string | null;
    voiceEnabled: boolean;
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
    settingsOpen: boolean;
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

  // Voice actions
  joinVoiceChannel: (channelName: string) => Promise<void>;
  leaveVoiceChannel: () => Promise<void>;
  toggleMute: () => Promise<void>;
  toggleDeafen: () => Promise<void>;
  setVoiceParticipants: (participants: VoiceParticipant[]) => void;
  updateSpeaking: (peerId: string, speaking: boolean) => void;

  // Relay hosting actions
  startHostedRelay: (port: number, voiceEnabled: boolean) => Promise<void>;
  stopHostedRelay: () => Promise<void>;
  toggleSettings: () => void;

  // Group/channel management
  createGroup: (name: string) => Promise<void>;
  deleteGroup: (groupId: string) => Promise<void>;
  renameGroup: (groupId: string, newName: string) => Promise<void>;
  createChannel: (name: string, kind: string) => Promise<void>;
  deleteChannel: (name: string) => Promise<void>;

  // File/media actions
  sendFile: (filePath: string) => Promise<void>;
  sendFileBytes: (data: number[], filename: string) => Promise<void>;

  // Async actions
  hydrateFromBackend: () => Promise<void>;
  sendMessage: (text: string, replyToId?: string) => Promise<void>;
  switchGroup: (groupId: string) => Promise<void>;
  switchChannel: (channelName: string) => Promise<void>;
}

// ── Backend message mapping ──

interface BackendMessageInfo {
  id: string;
  senderId: string;
  senderName: string;
  content: string;
  timestamp: number;
  isSelf: boolean;
  channelId: string | null;
  replyToSender: string | null;
  replyToPreview: string | null;
  kindType?: string;
  blobId?: string;
  width?: number;
  height?: number;
  thumbnailB64?: string;
  filename?: string;
  sizeBytes?: number;
  durationSecs?: number;
  waveform?: number[];
}

function backendToChat(m: BackendMessageInfo): ChatMessage {
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
      kind = { type: 'text', content: m.content };
  }

  return {
    id: m.id,
    senderId: m.senderId,
    senderName: m.senderName,
    senderRole: m.isSelf ? 'owner' : 'member',
    kind,
    timestamp: m.timestamp * 1000,
    edited: false,
    pinned: false,
    ephemeral: false,
    expiresAt: null,
    replyTo: m.replyToSender
      ? { id: '', senderName: m.replyToSender, preview: m.replyToPreview ?? '' }
      : null,
    reactions: [],
    isSelf: m.isSelf,
  };
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
    { id: 'cat-voice', name: 'Voice Channels', position: 1, collapsed: false },
  ],
  activeChannelId: null,

  members: [],
  messages: [],

  connection: {
    state: 'disconnected',
    relayConnected: false,
    peerCount: 0,
  },

  voice: {
    inRoom: false,
    roomId: null,
    channelName: null,
    isMuted: false,
    isDeafened: false,
    participants: [],
  },

  relayHosting: {
    active: false,
    addr: null,
    voiceEnabled: false,
  },

  ui: {
    screen: 'loading',
    memberListOpen: true,
    searchActive: false,
    searchQuery: '',
    showPins: false,
    replyingTo: null,
    editingMessage: null,
    settingsOpen: false,
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

  // ── Relay hosting + settings ──

  startHostedRelay: async (port, voiceEnabled) => {
    try {
      const status = await invoke<{ hosting: boolean; addr: string | null; voiceEnabled: boolean }>(
        'start_hosted_relay',
        { port, voiceEnabled },
      );
      set({
        relayHosting: {
          active: status.hosting,
          addr: status.addr,
          voiceEnabled: status.voiceEnabled,
        },
      });
      // Auto-connect to the local relay
      if (status.addr) {
        await invoke('connect_relay', { addr: status.addr });
      }
    } catch (e) {
      console.error('Failed to start hosted relay:', e);
      throw e;
    }
  },

  stopHostedRelay: async () => {
    try {
      await invoke('stop_hosted_relay');
      set({ relayHosting: { active: false, addr: null, voiceEnabled: false } });
    } catch (e) {
      console.error('Failed to stop hosted relay:', e);
    }
  },

  toggleSettings: () =>
    set((s) => ({ ui: { ...s.ui, settingsOpen: !s.ui.settingsOpen } })),

  // ── Voice actions ──

  joinVoiceChannel: async (channelName) => {
    try {
      await invoke('voice_join', { channelName });
      set({
        voice: {
          inRoom: true,
          roomId: null, // Will be set when we receive the offer
          channelName,
          isMuted: false,
          isDeafened: false,
          participants: [],
        },
      });
    } catch (e) {
      console.error('Failed to join voice channel:', e);
    }
  },

  leaveVoiceChannel: async () => {
    try {
      webRTCManager.disconnect();
      await invoke('voice_leave');
      set({
        voice: {
          inRoom: false,
          roomId: null,
          channelName: null,
          isMuted: false,
          isDeafened: false,
          participants: [],
        },
      });
    } catch (e) {
      console.error('Failed to leave voice channel:', e);
    }
  },

  toggleMute: async () => {
    const { voice } = get();
    const newMuted = !voice.isMuted;
    try {
      await invoke('voice_set_mute', { muted: newMuted });
      webRTCManager.setMuted(newMuted);
      set({ voice: { ...voice, isMuted: newMuted } });
    } catch (e) {
      console.error('Failed to toggle mute:', e);
    }
  },

  toggleDeafen: async () => {
    const { voice } = get();
    const newDeafened = !voice.isDeafened;
    try {
      await invoke('voice_set_deafen', { deafened: newDeafened });
      if (newDeafened) {
        webRTCManager.setMuted(true);
      } else {
        webRTCManager.setMuted(voice.isMuted);
      }
      set({
        voice: {
          ...voice,
          isDeafened: newDeafened,
          isMuted: newDeafened ? true : voice.isMuted,
        },
      });
    } catch (e) {
      console.error('Failed to toggle deafen:', e);
    }
  },

  setVoiceParticipants: (participants) => {
    set((s) => ({ voice: { ...s.voice, participants } }));
  },

  updateSpeaking: (peerId, speaking) => {
    set((s) => ({
      voice: {
        ...s.voice,
        participants: s.voice.participants.map((p) =>
          p.peerId === peerId ? { ...p, isSpeaking: speaking } : p,
        ),
      },
    }));
  },

  // ── Group & channel management ──

  createGroup: async (name) => {
    try {
      const result = await invoke<{ id: string; name: string; memberCount: number; unreadCount: number }>(
        'create_group',
        { name },
      );
      const newGroup: Group = { id: result.id, name: result.name, description: '', unreadCount: 0 };
      set((s) => ({ groups: [...s.groups, newGroup] }));
      // Switch to the new group
      await get().switchGroup(result.id);
    } catch (e) {
      console.error('Failed to create group:', e);
      throw e;
    }
  },

  deleteGroup: async (groupId) => {
    try {
      await invoke('delete_group', { groupId });
      const groups = get().groups.filter((g) => g.id !== groupId);
      const newActiveId = groups.length > 0 ? groups[0].id : null;
      set({ groups, activeGroupId: newActiveId });
      if (newActiveId) {
        await get().switchGroup(newActiveId);
      } else {
        set({ channels: [], messages: [] });
      }
    } catch (e) {
      console.error('Failed to delete group:', e);
      throw e;
    }
  },

  renameGroup: async (groupId, newName) => {
    try {
      await invoke('rename_group', { groupId, newName });
      set((s) => ({
        groups: s.groups.map((g) => (g.id === groupId ? { ...g, name: newName } : g)),
      }));
    } catch (e) {
      console.error('Failed to rename group:', e);
      throw e;
    }
  },

  createChannel: async (name, kind) => {
    try {
      await invoke('create_channel', { name, kind });
      const channelName = name.toLowerCase().replace(/ /g, '-');
      const categoryId = kind === 'voice' ? 'cat-voice' : 'cat-text';
      set((s) => ({
        channels: [
          ...s.channels,
          {
            id: `ch-${channelName}`,
            name: channelName,
            kind: kind as Channel['kind'],
            categoryId,
            position: s.channels.filter((c) => c.categoryId === categoryId).length,
            unread: false,
          },
        ],
      }));
    } catch (e) {
      console.error('Failed to create channel:', e);
      throw e;
    }
  },

  deleteChannel: async (name) => {
    try {
      await invoke('delete_channel', { name });
      set((s) => {
        const channels = s.channels.filter((c) => c.name !== name);
        const newActive = s.activeChannelId === `ch-${name}` && channels.length > 0
          ? channels[0].id
          : s.activeChannelId;
        return { channels, activeChannelId: newActive };
      });
    } catch (e) {
      console.error('Failed to delete channel:', e);
      throw e;
    }
  },

  // ── File / media actions ──

  sendFile: async (filePath) => {
    try {
      const result = await invoke<BackendMessageInfo>('send_file', { filePath });
      const msg = backendToChat(result);
      set((s) => ({ messages: [...s.messages, msg] }));
    } catch (e) {
      console.error('Failed to send file:', e);
      throw e;
    }
  },

  sendFileBytes: async (data, filename) => {
    try {
      const result = await invoke<BackendMessageInfo>('send_file_bytes', { data, filename });
      const msg = backendToChat(result);
      set((s) => ({ messages: [...s.messages, msg] }));
    } catch (e) {
      console.error('Failed to send file bytes:', e);
      throw e;
    }
  },

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
      // Add default voice channels
      mappedChannels.push({
        id: 'ch-voice-general',
        name: 'General',
        kind: 'voice',
        categoryId: 'cat-voice',
        position: 0,
        unread: false,
      });
      const activeChannelId = mappedChannels.length > 0 ? mappedChannels[0].id : null;

      if (channels.length > 0) {
        await invoke('set_active_channel', { name: channels[0].name });
      }

      // Load messages
      const msgs = await invoke<BackendMessageInfo[]>('get_messages', { limit: 200 });
      const mappedMessages: ChatMessage[] = msgs.map(backendToChat);

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

      const msgs = await invoke<BackendMessageInfo[]>('get_messages', { limit: 200 });
      const mappedMessages: ChatMessage[] = msgs.map(backendToChat);

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
