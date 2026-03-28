export interface VoiceParticipant {
  peerId: string;
  displayName: string;
  isMuted: boolean;
  isSpeaking: boolean;
}

export interface VoiceState {
  inRoom: boolean;
  roomId: string | null;
  channelName: string | null;
  isMuted: boolean;
  isDeafened: boolean;
  participants: VoiceParticipant[];
}

export interface VoiceOfferEvent {
  roomId: string;
  participantId: number;
  sdp: string;
  voiceEndpoint: string;
  participants: string[];
}

export interface VoiceIceCandidateEvent {
  roomId: string;
  participantId: number;
  candidate: string;
}

export interface VoiceParticipantEvent {
  roomId: string;
  peerId: string;
}

export interface VoiceSpeakingEvent {
  roomId: string;
  peerId: string;
  audioLevel: number;
  speaking: boolean;
}
