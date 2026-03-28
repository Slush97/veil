import { apiFetch } from './client';

export interface Channel {
  id: string;
  server_id: string;
  category_id: string | null;
  name: string;
  kind: 'text' | 'voice';
  position: number;
  created_at: string;
}

export interface ChannelListResponse {
  categories: {
    id: string;
    name: string;
    position: number;
    channels: Channel[];
  }[];
  uncategorized: Channel[];
}

export function listChannels(serverId: string) {
  return apiFetch<ChannelListResponse>('GET', `/api/servers/${serverId}/channels`);
}

export function createChannel(serverId: string, name: string, kind?: 'text' | 'voice', categoryId?: string) {
  return apiFetch<Channel>('POST', `/api/servers/${serverId}/channels`, {
    name,
    kind: kind ?? 'text',
    category_id: categoryId,
  });
}

export function deleteChannel(id: string) {
  return apiFetch<{ deleted: boolean }>('DELETE', `/api/channels/${id}`);
}
