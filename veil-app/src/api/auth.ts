import { apiFetch, setServerUrl, setToken } from './client';

interface AuthResponse {
  token: string;
  user: User;
}

export interface User {
  id: string;
  username: string;
  display_name: string;
  avatar_path: string | null;
  bio: string | null;
  status: string | null;
  created_at: string;
}

export async function register(
  serverUrl: string,
  username: string,
  password: string,
): Promise<AuthResponse> {
  setServerUrl(serverUrl);
  const res = await apiFetch<AuthResponse>('POST', '/api/auth/register', {
    username,
    password,
  });
  setToken(res.token);
  return res;
}

export async function login(
  serverUrl: string,
  username: string,
  password: string,
): Promise<AuthResponse> {
  setServerUrl(serverUrl);
  const res = await apiFetch<AuthResponse>('POST', '/api/auth/login', {
    username,
    password,
  });
  setToken(res.token);
  return res;
}

export async function getMe(): Promise<User> {
  return apiFetch<User>('GET', '/api/auth/me');
}
