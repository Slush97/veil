const LS_SERVER_URL = 'veil_server_url';
const LS_TOKEN = 'veil_token';

export function getServerUrl(): string | null {
  return localStorage.getItem(LS_SERVER_URL);
}

export function setServerUrl(url: string): void {
  // Strip trailing slash
  localStorage.setItem(LS_SERVER_URL, url.replace(/\/+$/, ''));
}

export function getToken(): string | null {
  return localStorage.getItem(LS_TOKEN);
}

export function setToken(token: string): void {
  localStorage.setItem(LS_TOKEN, token);
}

export function clearAuth(): void {
  localStorage.removeItem(LS_TOKEN);
}

export async function apiFetch<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const serverUrl = getServerUrl();
  if (!serverUrl) throw new Error('Server URL not configured');

  const headers: Record<string, string> = {};
  const token = getToken();
  if (token) headers['Authorization'] = `Bearer ${token}`;

  if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
  }

  const res = await fetch(`${serverUrl}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  if (res.status === 401) {
    clearAuth();
    window.location.reload();
    throw new Error('Session expired');
  }

  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(text || `HTTP ${res.status}`);
  }

  return res.json() as Promise<T>;
}
