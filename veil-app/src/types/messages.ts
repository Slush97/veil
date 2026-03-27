export type MessageKind =
  | { type: 'text'; content: string }
  | { type: 'image'; blobId: string; width: number; height: number; thumbnailUrl?: string }
  | { type: 'file'; blobId: string; filename: string; sizeBytes: number }
  | { type: 'audio'; blobId: string; durationSecs: number; waveform: number[] }
  | { type: 'system'; content: string };

export interface Reaction {
  emoji: string;
  count: number;
  reacted: boolean;
}

export interface ChatMessage {
  id: string;
  senderId: string;
  senderName: string;
  senderRole: string;
  kind: MessageKind;
  timestamp: number;
  edited: boolean;
  pinned: boolean;
  ephemeral: boolean;
  expiresAt: number | null;
  replyTo: {
    id: string;
    senderName: string;
    preview: string;
  } | null;
  reactions: Reaction[];
  isSelf: boolean;
}
