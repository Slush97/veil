import { useState } from 'react';
import { Copy, Check, Shield } from 'lucide-react';
import { useAppStore } from '../../store/appStore';
import styles from './RecoveryPhrase.module.css';

export function RecoveryPhrase() {
  const setScreen = useAppStore((s) => s.setScreen);
  const recoveryPhrase = useAppStore((s) => s.recoveryPhrase);
  const [copied, setCopied] = useState(false);
  const [confirmed, setConfirmed] = useState(false);

  const words = recoveryPhrase?.split(' ') ?? [];

  const handleCopy = async () => {
    if (recoveryPhrase) {
      await navigator.clipboard.writeText(recoveryPhrase);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleContinue = () => {
    // Clear the recovery phrase from state and go to chat
    useAppStore.setState({ recoveryPhrase: null });
    setScreen('chat');
  };

  return (
    <div className={styles.container}>
      <div className={styles.card}>
        <div className={styles.logo}>
          <Shield size={48} />
        </div>
        <h1 className={styles.title}>Save Your Recovery Phrase</h1>
        <p className={styles.subtitle}>
          Write down these 12 words in order. This is the only way to recover your account
          if you lose your passphrase.
        </p>

        <div className={styles.phraseGrid}>
          {words.map((word, i) => (
            <div key={i} className={styles.word}>
              <span className={styles.wordNumber}>{i + 1}</span>
              <span className={styles.wordText}>{word}</span>
            </div>
          ))}
        </div>

        <button className={styles.copyButton} onClick={handleCopy}>
          {copied ? <Check size={16} /> : <Copy size={16} />}
          {copied ? 'Copied!' : 'Copy to clipboard'}
        </button>

        <label className={styles.confirmLabel}>
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(e) => setConfirmed(e.target.checked)}
          />
          <span>I have saved my recovery phrase in a safe place</span>
        </label>

        <button
          className={styles.primaryButton}
          onClick={handleContinue}
          disabled={!confirmed}
        >
          Continue to Veil
        </button>
      </div>
    </div>
  );
}
