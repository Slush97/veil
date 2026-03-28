export interface IdentityInfo {
  masterPeerId: string;
  deviceName: string;
  username: string | null;
  displayName: string;
}

export type ConnectionState =
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'failed';

export type Screen = 'loading' | 'setup' | 'recovery' | 'onboarding' | 'chat' | 'settings';
