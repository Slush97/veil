import { apiFetch } from './client';

export interface FileInfo {
  id: string;
  filename: string;
  mime_type: string;
  size_bytes: number;
  width: number | null;
  height: number | null;
  duration_secs: number | null;
  waveform: number[] | null;
  has_thumbnail: boolean;
}

export interface MessageAuthor {
  id: string;
  username: string;
  display_name: string;
  avatar_path: string | null;
}

export interface ServerMessage {
  id: string;
  channel_id: string;
  author: MessageAuthor;
  content: string | null;
  reply_to_id: string | null;
  files: FileInfo[];
  edited_at: string | null;
  created_at: string;
}

export function listMessages(channelId: string, opts?: { limit?: number; before?: string }) {
  const params = new URLSearchParams();
  if (opts?.limit) params.set('limit', String(opts.limit));
  if (opts?.before) params.set('before', opts.before);
  const qs = params.toString();
  return apiFetch<ServerMessage[]>('GET', `/api/channels/${channelId}/messages${qs ? `?${qs}` : ''}`);
}

export function sendMessage(channelId: string, content: string, replyToId?: string) {
  return apiFetch<ServerMessage>('POST', `/api/channels/${channelId}/messages`, {
    content,
    reply_to_id: replyToId ?? null,
  });
}

export function editMessage(id: string, content: string) {
  return apiFetch<ServerMessage>('PUT', `/api/messages/${id}`, { content });
}

export function deleteMessage(id: string) {
  return apiFetch<{ deleted: boolean }>('DELETE', `/api/messages/${id}`);
}
