import { Mic, MicOff, Headphones, HeadphoneOff, PhoneOff } from 'lucide-react';
import clsx from 'clsx';
import { useAppStore } from '../../store/appStore';
import styles from './VoiceControls.module.css';

export function VoiceControls() {
  const voice = useAppStore((s) => s.voice);
  const toggleMute = useAppStore((s) => s.toggleMute);
  const toggleDeafen = useAppStore((s) => s.toggleDeafen);
  const leaveVoiceChannel = useAppStore((s) => s.leaveVoiceChannel);

  if (!voice.inRoom) return null;

  return (
    <div className={styles.container}>
      {/* Connection info */}
      <div className={styles.info}>
        <div className={styles.connected}>
          <span className={styles.dot} />
          Voice Connected
        </div>
        <div className={styles.channelName}>{voice.channelName}</div>
      </div>

      {/* Participant avatars */}
      {voice.participants.length > 0 && (
        <div className={styles.participants}>
          {voice.participants.map((p) => (
            <div
              key={p.peerId}
              className={clsx(styles.participant, p.isSpeaking && styles.speaking)}
              title={p.displayName}
            >
              <span className={styles.participantName}>
                {p.displayName.substring(0, 6)}
              </span>
              {p.isMuted && <MicOff size={10} className={styles.mutedIcon} />}
            </div>
          ))}
        </div>
      )}

      {/* Controls */}
      <div className={styles.controls}>
        <button
          className={clsx(styles.controlBtn, voice.isMuted && styles.active)}
          onClick={() => toggleMute()}
          title={voice.isMuted ? 'Unmute' : 'Mute'}
        >
          {voice.isMuted ? <MicOff size={18} /> : <Mic size={18} />}
        </button>
        <button
          className={clsx(styles.controlBtn, voice.isDeafened && styles.active)}
          onClick={() => toggleDeafen()}
          title={voice.isDeafened ? 'Undeafen' : 'Deafen'}
        >
          {voice.isDeafened ? <HeadphoneOff size={18} /> : <Headphones size={18} />}
        </button>
        <button
          className={clsx(styles.controlBtn, styles.disconnect)}
          onClick={() => leaveVoiceChannel()}
          title="Disconnect"
        >
          <PhoneOff size={18} />
        </button>
      </div>
    </div>
  );
}
