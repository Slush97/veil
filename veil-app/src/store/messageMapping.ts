import type { ChatMessage } from '../types/messages';
import type { ServerMessage } from '../api/messages';
import { getFileUrl, getThumbnailUrl } from '../api/files';

/**
 * Convert a server Message response into the frontend ChatMessage shape.
 */
export function serverMessageToChat(m: ServerMessage, currentUserId: string): ChatMessage {
  let kind: ChatMessage['kind'];

  if (m.files && m.files.length > 0) {
    const f = m.files[0];
    const mime = f.mime_type ?? '';

    if (mime.startsWith('image/')) {
      kind = {
        type: 'image',
        blobId: f.id,
        width: f.width ?? 0,
        height: f.height ?? 0,
        thumbnailUrl: f.has_thumbnail ? getThumbnailUrl(f.id) : undefined,
      };
    } else if (mime.startsWith('video/')) {
      kind = {
        type: 'video',
        blobId: f.id,
        durationSecs: f.duration_secs ?? 0,
        thumbnailUrl: f.has_thumbnail ? getThumbnailUrl(f.id) : undefined,
      };
    } else if (mime.startsWith('audio/')) {
      kind = {
        type: 'audio',
        blobId: f.id,
        durationSecs: f.duration_secs ?? 0,
        waveform: f.waveform ?? [],
      };
    } else {
      kind = {
        type: 'file',
        blobId: f.id,
        filename: f.filename ?? 'file',
        sizeBytes: f.size_bytes ?? 0,
      };
    }
  } else {
    kind = { type: 'text', content: m.content ?? '' };
  }

  const isSelf = m.author.id === currentUserId;

  return {
    id: m.id,
    senderId: m.author.id,
    senderName: m.author.display_name || m.author.username,
    senderRole: isSelf ? 'owner' : 'member',
    kind,
    timestamp: new Date(m.created_at).getTime(),
    edited: m.edited_at !== null,
    pinned: false,
    ephemeral: false,
    expiresAt: null,
    replyTo: null, // Server doesn't inline reply content yet
    reactions: [],
    isSelf,
  };
}
