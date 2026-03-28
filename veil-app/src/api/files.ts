import { getServerUrl, getToken } from './client';

export function getFileUrl(fileId: string): string {
  return `${getServerUrl()}/api/files/${fileId}?token=${getToken()}`;
}

export function getThumbnailUrl(fileId: string): string {
  return `${getServerUrl()}/api/files/${fileId}/thumbnail?token=${getToken()}`;
}

export async function uploadFile(
  _channelId: string,
  _file: File,
  _filename: string,
): Promise<never> {
  throw new Error('File upload not implemented yet (Phase 4)');
}
