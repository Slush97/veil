import { create } from 'zustand';
import type { Screen } from '../types/identity';
import type { Group, Channel, Category, Member } from '../types/groups';
import type { ChatMessage } from '../types/messages';
import type { VoiceParticipant, VoiceState } from '../types/voice';
import type { User } from '../api/auth';
import { webRTCManager } from '../voice/WebRTCManager';
import { serverMessageToChat } from './messageMapping';
import * as api from '../api';

type WsState = 'disconnected' | 'connecting' | 'connected';

interface AppState {
  // Auth
  auth: {
    token: string | null;
    serverUrl: string | null;
    user: User | null;
  };

  // Identity (derived from auth.user for UI compat)
  identity: {
    masterPeerId: string | null;
    username: string | null;
    displayName: string;
    bio: string;
    status: string;
    isSetUp: boolean;
  };

  // Groups (servers)
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
    wsState: WsState;
  };

  // Voice
  voice: VoiceState;

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

  toggleSettings: () => void;

  // Group/channel management
  createGroup: (name: string) => Promise<void>;
  deleteGroup: (groupId: string) => Promise<void>;
  renameGroup: (groupId: string, newName: string) => Promise<void>;
  createChannel: (name: string, kind: string) => Promise<void>;
  deleteChannel: (channelId: string) => Promise<void>;

  // Async actions
  hydrateFromBackend: () => Promise<void>;
  sendMessage: (text: string, replyToId?: string) => Promise<void>;
  switchGroup: (groupId: string) => Promise<void>;
  switchChannel: (channelId: string) => Promise<void>;
}

// ── Store ──

export const useAppStore = create<AppState>((set, get) => ({
  auth: {
    token: api.getToken(),
    serverUrl: api.getServerUrl(),
    user: null,
  },

  identity: {
    masterPeerId: null,
    username: null,
    displayName: '',
    bio: '',
    status: '',
    isSetUp: false,
  },

  groups: [],
  activeGroupId: null,

  channels: [],
  categories: [],
  activeChannelId: null,

  members: [],
  messages: [],

  connection: {
    wsState: 'disconnected',
  },

  voice: {
    inRoom: false,
    roomId: null,
    channelName: null,
    isMuted: false,
    isDeafened: false,
    participants: [],
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
  addMessage: (msg) =>
    set((s) => {
      if (s.messages.some((m) => m.id === msg.id)) return s;
      return { messages: [...s.messages, msg] };
    }),
  setTheme: (theme) => set((s) => ({ ui: { ...s.ui, theme } })),

  toggleSettings: () =>
    set((s) => ({ ui: { ...s.ui, settingsOpen: !s.ui.settingsOpen } })),

  // ── Voice actions ──

  joinVoiceChannel: async (channelName) => {
    try {
      api.wsSend('voice_join', { channel_name: channelName });
      set({
        voice: {
          inRoom: true,
          roomId: null,
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
      api.wsSend('voice_leave', {});
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
    webRTCManager.setMuted(newMuted);
    set({ voice: { ...voice, isMuted: newMuted } });
  },

  toggleDeafen: async () => {
    const { voice } = get();
    const newDeafened = !voice.isDeafened;
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
      const server = await api.createServer(name);
      const newGroup: Group = {
        id: server.id,
        name: server.name,
        description: '',
        unreadCount: 0,
      };
      set((s) => ({ groups: [...s.groups, newGroup] }));
      await get().switchGroup(server.id);
    } catch (e) {
      console.error('Failed to create group:', e);
      throw e;
    }
  },

  deleteGroup: async (groupId) => {
    try {
      await api.deleteServer(groupId);
      const groups = get().groups.filter((g) => g.id !== groupId);
      const newActiveId = groups.length > 0 ? groups[0].id : null;
      set({ groups, activeGroupId: newActiveId });
      if (newActiveId) {
        await get().switchGroup(newActiveId);
      } else {
        set({ channels: [], categories: [], messages: [], members: [] });
      }
    } catch (e) {
      console.error('Failed to delete group:', e);
      throw e;
    }
  },

  renameGroup: async (groupId, newName) => {
    try {
      await api.updateServer(groupId, newName);
      set((s) => ({
        groups: s.groups.map((g) => (g.id === groupId ? { ...g, name: newName } : g)),
      }));
    } catch (e) {
      console.error('Failed to rename group:', e);
      throw e;
    }
  },

  createChannel: async (name, kind) => {
    const { activeGroupId } = get();
    if (!activeGroupId) return;
    try {
      const ch = await api.createChannel(activeGroupId, name, kind as 'text' | 'voice');
      set((s) => ({
        channels: [
          ...s.channels,
          {
            id: ch.id,
            name: ch.name,
            kind: ch.kind as Channel['kind'],
            categoryId: ch.category_id,
            position: ch.position,
            unread: false,
          },
        ],
      }));
    } catch (e) {
      console.error('Failed to create channel:', e);
      throw e;
    }
  },

  deleteChannel: async (channelId) => {
    try {
      await api.deleteChannel(channelId);
      set((s) => {
        const channels = s.channels.filter((c) => c.id !== channelId);
        const newActive =
          s.activeChannelId === channelId && channels.length > 0
            ? channels[0].id
            : s.activeChannelId;
        return { channels, activeChannelId: newActive };
      });
    } catch (e) {
      console.error('Failed to delete channel:', e);
      throw e;
    }
  },

  // ── Async actions that call REST API ──

  hydrateFromBackend: async () => {
    try {
      const userId = get().auth.user?.id ?? '';

      // Load servers
      const servers = await api.listServers();
      const mappedGroups: Group[] = servers.map((s) => ({
        id: s.id,
        name: s.name,
        description: '',
        unreadCount: 0,
      }));

      const activeGroupId = mappedGroups.length > 0 ? mappedGroups[0].id : null;

      let mappedChannels: Channel[] = [];
      let mappedCategories: Category[] = [];
      let activeChannelId: string | null = null;
      let mappedMessages: ChatMessage[] = [];
      let mappedMembers: Member[] = [];

      if (activeGroupId) {
        // Load channels
        const channelResp = await api.listChannels(activeGroupId);

        mappedCategories = channelResp.categories.map((cat) => ({
          id: cat.id,
          name: cat.name,
          position: cat.position,
          collapsed: false,
        }));

        for (const cat of channelResp.categories) {
          for (const ch of cat.channels) {
            mappedChannels.push({
              id: ch.id,
              name: ch.name,
              kind: ch.kind as Channel['kind'],
              categoryId: cat.id,
              position: ch.position,
              unread: false,
            });
          }
        }
        for (const ch of channelResp.uncategorized) {
          mappedChannels.push({
            id: ch.id,
            name: ch.name,
            kind: ch.kind as Channel['kind'],
            categoryId: null,
            position: ch.position,
            unread: false,
          });
        }

        // Pick first text channel
        const firstText = mappedChannels.find((c) => c.kind === 'text');
        activeChannelId = firstText?.id ?? (mappedChannels[0]?.id ?? null);

        // Load messages for the active channel
        if (activeChannelId) {
          const msgs = await api.listMessages(activeChannelId, { limit: 50 });
          mappedMessages = msgs.map((m) => serverMessageToChat(m, userId));
        }

        // Load members
        const members = await api.listMembers(activeGroupId);
        mappedMembers = members.map((m) => ({
          peerId: m.user_id,
          displayName: m.display_name || m.username,
          role: m.role as Member['role'],
          status: 'online' as const,
          bio: '',
          statusText: '',
          isTyping: false,
          isSelf: m.user_id === userId,
        }));
      }

      set({
        groups: mappedGroups,
        activeGroupId,
        channels: mappedChannels,
        categories: mappedCategories,
        activeChannelId,
        messages: mappedMessages,
        members: mappedMembers,
      });
    } catch (e) {
      console.error('Failed to hydrate from backend:', e);
    }
  },

  sendMessage: async (text, replyToId) => {
    const { activeChannelId, auth } = get();
    if (!activeChannelId) return;
    try {
      const result = await api.sendMessage(activeChannelId, text, replyToId);
      const msg = serverMessageToChat(result, auth.user?.id ?? '');

      set((s) => {
        // Dedup
        if (s.messages.some((m) => m.id === msg.id)) return { ui: { ...s.ui, replyingTo: null } };
        return {
          messages: [...s.messages, msg],
          ui: { ...s.ui, replyingTo: null },
        };
      });
    } catch (e) {
      console.error('Failed to send message:', e);
    }
  },

  switchGroup: async (groupId) => {
    const { auth } = get();
    const userId = auth.user?.id ?? '';
    try {
      set({ activeGroupId: groupId, messages: [], channels: [], categories: [] });

      // Load channels
      const channelResp = await api.listChannels(groupId);

      const mappedCategories: Category[] = channelResp.categories.map((cat) => ({
        id: cat.id,
        name: cat.name,
        position: cat.position,
        collapsed: false,
      }));

      const mappedChannels: Channel[] = [];
      for (const cat of channelResp.categories) {
        for (const ch of cat.channels) {
          mappedChannels.push({
            id: ch.id,
            name: ch.name,
            kind: ch.kind as Channel['kind'],
            categoryId: cat.id,
            position: ch.position,
            unread: false,
          });
        }
      }
      for (const ch of channelResp.uncategorized) {
        mappedChannels.push({
          id: ch.id,
          name: ch.name,
          kind: ch.kind as Channel['kind'],
          categoryId: null,
          position: ch.position,
          unread: false,
        });
      }

      const firstText = mappedChannels.find((c) => c.kind === 'text');
      const activeChannelId = firstText?.id ?? (mappedChannels[0]?.id ?? null);

      let mappedMessages: ChatMessage[] = [];
      if (activeChannelId) {
        const msgs = await api.listMessages(activeChannelId, { limit: 50 });
        mappedMessages = msgs.map((m) => serverMessageToChat(m, userId));
      }

      // Load members
      const members = await api.listMembers(groupId);
      const mappedMembers: Member[] = members.map((m) => ({
        peerId: m.user_id,
        displayName: m.display_name || m.username,
        role: m.role as Member['role'],
        status: 'online' as const,
        bio: '',
        statusText: '',
        isTyping: false,
        isSelf: m.user_id === userId,
      }));

      set({
        channels: mappedChannels,
        categories: mappedCategories,
        activeChannelId,
        messages: mappedMessages,
        members: mappedMembers,
      });
    } catch (e) {
      console.error('Failed to switch group:', e);
    }
  },

  switchChannel: async (channelId) => {
    const { auth } = get();
    const userId = auth.user?.id ?? '';
    try {
      set({ activeChannelId: channelId, messages: [] });

      const msgs = await api.listMessages(channelId, { limit: 50 });
      const mappedMessages = msgs.map((m) => serverMessageToChat(m, userId));
      set({ messages: mappedMessages });
    } catch (e) {
      console.error('Failed to switch channel:', e);
    }
  },
}));
