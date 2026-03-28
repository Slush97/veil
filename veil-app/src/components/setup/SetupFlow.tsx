import { useState } from 'react';
import { Shield, Eye, EyeOff, Loader2 } from 'lucide-react';
import { useAppStore } from '../../store/appStore';
import { register, login, getServerUrl, connectWs } from '../../api';
import styles from './SetupFlow.module.css';

export function SetupFlow() {
  const setScreen = useAppStore((s) => s.setScreen);
  const setIdentity = useAppStore((s) => s.setIdentity);
  const serverUrl = getServerUrl();

  const [mode, setMode] = useState<'choose' | 'create' | 'login'>('choose');
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
    if (!serverUrl) {
      setError('No server URL configured');
      return;
    }
    setLoading(true);
    setError('');
    try {
      const { token, user } = await register(serverUrl, username.trim(), password.trim());
      useAppStore.setState({
        auth: { token, serverUrl, user },
      });
      setIdentity({
        masterPeerId: user.id,
        username: user.username,
        displayName: user.display_name,
        bio: user.bio ?? '',
        status: user.status ?? '',
        isSetUp: true,
      });
      connectWs(token);
      setScreen('onboarding');
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleLogin = async () => {
    if (!username.trim() || !password.trim()) {
      setError('Username and password are required');
      return;
    }
    if (!serverUrl) {
      setError('No server URL configured');
      return;
    }
    setLoading(true);
    setError('');
    try {
      const { token, user } = await login(serverUrl, username.trim(), password.trim());
      useAppStore.setState({
        auth: { token, serverUrl, user },
      });
      setIdentity({
        masterPeerId: user.id,
        username: user.username,
        displayName: user.display_name,
        bio: user.bio ?? '',
        status: user.status ?? '',
        isSetUp: true,
      });
      connectWs(token);
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
        <p className={styles.subtitle}>Connected to {serverUrl}</p>

        {mode === 'choose' && (
          <div className={styles.choices}>
            <button className={styles.primaryButton} onClick={() => setMode('create')}>
              Create Account
            </button>
            <button className={styles.secondaryButton} onClick={() => setMode('login')}>
              Sign In
            </button>
            <button
              className={styles.ghostButton}
              onClick={() => {
                useAppStore.getState().setScreen('server-connect');
              }}
            >
              Change Server
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
            <p className={styles.loginHint}>Welcome back. Sign in to your account.</p>

            <label className={styles.label}>Username</label>
            <input
              className={styles.input}
              type="text"
              placeholder="Your username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoFocus
            />

            <label className={styles.label}>Password</label>
            <div className={styles.inputGroup}>
              <input
                className={styles.input}
                type={showPass ? 'text' : 'password'}
                placeholder="Enter your password"
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
              onClick={handleLogin}
              disabled={loading}
            >
              {loading ? <Loader2 size={16} className={styles.spinner} /> : null}
              Sign In
            </button>
            <button className={styles.ghostButton} onClick={() => { setMode('choose'); setError(''); }}>
              Back
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
