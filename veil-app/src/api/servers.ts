import { apiFetch } from './client';

export interface Server {
  id: string;
  name: string;
  owner_id: string;
  icon_path: string | null;
  member_count: number;
  created_at: string;
}

export interface Member {
  user_id: string;
  username: string;
  display_name: string;
  avatar_path: string | null;
  role: string;
  joined_at: string;
}

export interface Invite {
  code: string;
  server_id: string;
  server_name: string;
  member_count: number;
  creator_id: string;
  max_uses: number | null;
  use_count: number;
  expires_at: string | null;
  created_at: string;
}

export function listServers() {
  return apiFetch<Server[]>('GET', '/api/servers');
}

export function createServer(name: string) {
  return apiFetch<Server>('POST', '/api/servers', { name });
}

export function updateServer(id: string, name: string) {
  return apiFetch<Server>('PUT', `/api/servers/${id}`, { name });
}

export function deleteServer(id: string) {
  return apiFetch<{ deleted: boolean }>('DELETE', `/api/servers/${id}`);
}

export function listMembers(serverId: string) {
  return apiFetch<Member[]>('GET', `/api/servers/${serverId}/members`);
}

export function removeMember(serverId: string, userId: string) {
  return apiFetch<{ removed: boolean }>('DELETE', `/api/servers/${serverId}/members/${userId}`);
}

export function createInvite(serverId: string, opts?: { max_uses?: number; expires_in_secs?: number }) {
  return apiFetch<Invite>('POST', `/api/servers/${serverId}/invites`, opts ?? {});
}

export function getInviteInfo(code: string) {
  return apiFetch<Invite>('GET', `/api/invites/${code}`);
}

export function acceptInvite(code: string) {
  return apiFetch<Invite>('POST', `/api/invites/${code}/accept`);
}
