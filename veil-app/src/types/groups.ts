export type Role = 'owner' | 'admin' | 'moderator' | 'member';

export type ChannelKind = 'text' | 'voice' | 'media';

export interface Group {
  id: string;
  name: string;
  description: string;
  unreadCount: number;
}

export interface Category {
  id: string;
  name: string;
  position: number;
  collapsed: boolean;
}

export interface Channel {
  id: string;
  name: string;
  kind: ChannelKind;
  categoryId: string | null;
  position: number;
  unread: boolean;
}

export interface Member {
  peerId: string;
  displayName: string;
  role: Role;
  status: 'online' | 'idle' | 'dnd' | 'offline';
  bio: string;
  statusText: string;
  isTyping: boolean;
  isSelf: boolean;
}
