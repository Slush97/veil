import { getServerUrl, getToken } from './client';

type WsState = 'connecting' | 'connected' | 'disconnected';
type WsHandler = (data: any) => void;

let ws: WebSocket | null = null;
let state: WsState = 'disconnected';
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = 1000;
const MAX_DELAY = 30000;

const handlers = new Map<string, Set<WsHandler>>();
const stateListeners = new Set<(s: WsState) => void>();

function notifyState(s: WsState) {
  state = s;
  for (const fn of stateListeners) fn(s);
}

export function onWsStateChange(fn: (s: WsState) => void): () => void {
  stateListeners.add(fn);
  return () => { stateListeners.delete(fn); };
}

export function getWsState(): WsState {
  return state;
}

export function connectWs(token?: string): void {
  if (ws) return;

  const serverUrl = getServerUrl();
  const jwt = token ?? getToken();
  if (!serverUrl || !jwt) return;

  const wsUrl = serverUrl.replace(/^http/, 'ws') + `/api/ws?token=${jwt}`;
  notifyState('connecting');

  const socket = new WebSocket(wsUrl);
  ws = socket;

  socket.onopen = () => {
    reconnectDelay = 1000;
    notifyState('connected');
  };

  socket.onmessage = (ev) => {
    try {
      const msg = JSON.parse(ev.data);
      const type: string = msg.type;
      const data = msg.data;
      const set = handlers.get(type);
      if (set) {
        for (const fn of set) fn(data);
      }
    } catch {
      // ignore malformed messages
    }
  };

  socket.onclose = () => {
    ws = null;
    notifyState('disconnected');
    scheduleReconnect();
  };

  socket.onerror = () => {
    socket.close();
  };
}

export function disconnectWs(): void {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  if (ws) {
    ws.onclose = null; // prevent reconnect
    ws.close();
    ws = null;
  }
  notifyState('disconnected');
}

export function wsSend(type: string, data: unknown): void {
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type, data }));
  }
}

export function wsOn(event: string, handler: WsHandler): void {
  let set = handlers.get(event);
  if (!set) {
    set = new Set();
    handlers.set(event, set);
  }
  set.add(handler);
}

export function wsOff(event: string, handler: WsHandler): void {
  const set = handlers.get(event);
  if (set) {
    set.delete(handler);
    if (set.size === 0) handlers.delete(event);
  }
}

function scheduleReconnect(): void {
  if (reconnectTimer) return;
  // Only reconnect if we have credentials
  if (!getServerUrl() || !getToken()) return;

  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectWs();
    reconnectDelay = Math.min(reconnectDelay * 2, MAX_DELAY);
  }, reconnectDelay);
}
