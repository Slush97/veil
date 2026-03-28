import { useState } from 'react';
import { Shield, Loader2, Server } from 'lucide-react';
import { setServerUrl } from '../../api';
import { useAppStore } from '../../store/appStore';
import styles from './SetupFlow.module.css';

export function ServerConnect() {
  const setScreen = useAppStore((s) => s.setScreen);
  const [url, setUrl] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleConnect = async () => {
    const trimmed = url.trim().replace(/\/+$/, '');
    if (!trimmed) {
      setError('Enter a server URL');
      return;
    }
    setLoading(true);
    setError('');
    try {
      const res = await fetch(`${trimmed}/api/health`);
      if (!res.ok) throw new Error(`Server responded with ${res.status}`);
      setServerUrl(trimmed);
      useAppStore.setState({
        auth: { ...useAppStore.getState().auth, serverUrl: trimmed },
      });
      setScreen('setup');
    } catch (e) {
      setError(`Could not reach server: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className={styles.container}>
      <div className={styles.card}>
        <div className={styles.logo}>
          <Shield size={48} />
        </div>
        <h1 className={styles.title}>Veil</h1>
        <p className={styles.subtitle}>Connect to a Veil server to get started.</p>

        <div className={styles.form} onKeyDown={(e) => e.key === 'Enter' && !loading && handleConnect()}>
          <label className={styles.label}>Server URL</label>
          <div className={styles.inputGroup}>
            <input
              className={styles.input}
              type="text"
              placeholder="http://localhost:3000"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              autoFocus
            />
          </div>

          {error && <div className={styles.error}>{error}</div>}

          <button
            className={styles.primaryButton}
            onClick={handleConnect}
            disabled={loading}
          >
            {loading ? <Loader2 size={16} className={styles.spinner} /> : <Server size={16} />}
            Connect
          </button>
        </div>
      </div>
    </div>
  );
}
