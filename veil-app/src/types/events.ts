import type { ChatMessage } from './messages';
import type { Group, Member } from './groups';

export interface PeerConnectedEvent {
  peerId: string;
  name: string;
}

export interface PeerDisconnectedEvent {
  peerId: string;
}

export interface MessageReceivedEvent {
  message: ChatMessage;
}

export interface TypingEvent {
  peerId: string;
  groupId: string;
  name: string;
}

export interface ConnectionStateEvent {
  state: string;
  message: string;
}

export interface InviteCreatedEvent {
  url: string;
}

export interface GroupJoinedEvent {
  group: Group;
  members: Member[];
}
