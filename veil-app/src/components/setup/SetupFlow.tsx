import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Shield, Eye, EyeOff, Loader2 } from 'lucide-react';
import { useAppStore } from '../../store/appStore';
import styles from './SetupFlow.module.css';

export function SetupFlow() {
  const setScreen = useAppStore((s) => s.setScreen);
  const setIdentity = useAppStore((s) => s.setIdentity);
  const hasExisting = useAppStore((s) => s.hasExistingIdentity);

  const [mode, setMode] = useState<'choose' | 'create' | 'login'>(
    hasExisting ? 'login' : 'choose',
  );
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [showPass, setShowPass] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleCreate = async () => {
    if (!username.trim() || !password.trim()) {
      setError('Username and password are required');
      return;
    }
    setLoading(true);
    setError('');
    try {
      const phrase = await invoke<string>('create_identity', {
        username: username.trim(),
        passphrase: password.trim(),
      });
      await invoke('start_network');
      useAppStore.setState({ recoveryPhrase: phrase });
      setScreen('recovery');
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleLogin = async () => {
    if (!password.trim()) {
      setError('Password is required');
      return;
    }
    setLoading(true);
    setError('');
    try {
      const info = await invoke<{
        masterPeerId: string;
        deviceName: string;
        username: string | null;
        displayName: string;
      }>('load_identity', { passphrase: password.trim() });
      setIdentity({
        masterPeerId: info.masterPeerId,
        username: info.username,
        displayName: info.displayName,
        bio: '',
        status: '',
        isSetUp: true,
      });
      await invoke('start_network');
      setScreen('chat');
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleDevLogin = async () => {
    setLoading(true);
    try {
      const info = await invoke<{
        masterPeerId: string;
        displayName: string;
      }>('dev_login');
      setIdentity({
        masterPeerId: info.masterPeerId,
        username: null,
        displayName: info.displayName,
        bio: '',
        status: '',
        isSetUp: true,
      });
      await invoke('start_network');
      setScreen('chat');
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !loading) {
      if (mode === 'create') handleCreate();
      else if (mode === 'login') handleLogin();
    }
  };

  return (
    <div className={styles.container}>
      <div className={styles.card}>
        <div className={styles.logo}>
          <Shield size={48} />
        </div>
        <h1 className={styles.title}>Veil</h1>
        <p className={styles.subtitle}>Encrypted, decentralized messaging</p>

        {mode === 'choose' && (
          <div className={styles.choices}>
            <button className={styles.primaryButton} onClick={() => setMode('create')}>
              Create Account
            </button>
            <button className={styles.secondaryButton} onClick={() => setMode('login')}>
              Sign In
            </button>
            <button className={styles.ghostButton} onClick={handleDevLogin} disabled={loading}>
              {loading ? <Loader2 size={16} className={styles.spinner} /> : null}
              Dev Login (no persistence)
            </button>
          </div>
        )}

        {mode === 'create' && (
          <div className={styles.form} onKeyDown={handleKeyDown}>
            <label className={styles.label}>Username</label>
            <input
              className={styles.input}
              type="text"
              placeholder="Choose a username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoFocus
            />

            <label className={styles.label}>Password</label>
            <div className={styles.inputGroup}>
              <input
                className={styles.input}
                type={showPass ? 'text' : 'password'}
                placeholder="Choose a password to protect your account"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
              <button
                className={styles.inputToggle}
                onClick={() => setShowPass(!showPass)}
                type="button"
              >
                {showPass ? <EyeOff size={16} /> : <Eye size={16} />}
              </button>
            </div>

            {error && <div className={styles.error}>{error}</div>}

            <button
              className={styles.primaryButton}
              onClick={handleCreate}
              disabled={loading}
            >
              {loading ? <Loader2 size={16} className={styles.spinner} /> : null}
              Create Account
            </button>
            <button className={styles.ghostButton} onClick={() => { setMode('choose'); setError(''); }}>
              Back
            </button>
          </div>
        )}

        {mode === 'login' && (
          <div className={styles.form} onKeyDown={handleKeyDown}>
            <p className={styles.loginHint}>Welcome back. Enter your password to unlock.</p>

            <label className={styles.label}>Password</label>
            <div className={styles.inputGroup}>
              <input
                className={styles.input}
                type={showPass ? 'text' : 'password'}
                placeholder="Enter your password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoFocus
              />
              <button
                className={styles.inputToggle}
                onClick={() => setShowPass(!showPass)}
                type="button"
              >
                {showPass ? <EyeOff size={16} /> : <Eye size={16} />}
              </button>
            </div>

            {error && <div className={styles.error}>{error}</div>}

            <button
              className={styles.primaryButton}
              onClick={handleLogin}
              disabled={loading}
            >
              {loading ? <Loader2 size={16} className={styles.spinner} /> : null}
              Unlock
            </button>
            {hasExisting && (
              <button className={styles.ghostButton} onClick={() => { setMode('choose'); setError(''); }}>
                Use a different account
              </button>
            )}
            {!hasExisting && (
              <button className={styles.ghostButton} onClick={() => { setMode('choose'); setError(''); }}>
                Back
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
