import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Shield, Plus, LogIn, Copy, Check, Loader2, ArrowLeft } from 'lucide-react';
import { useAppStore } from '../../store/appStore';
import styles from './SetupFlow.module.css';
import onbStyles from './Onboarding.module.css';

export function Onboarding() {
  const setScreen = useAppStore((s) => s.setScreen);
  const [mode, setMode] = useState<'choose' | 'create' | 'join'>('choose');
  const [serverName, setServerName] = useState('');
  const [inviteCode, setInviteCode] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  // Result from creating a server
  const [createResult, setCreateResult] = useState<{
    inviteCode: string;
    relayAddr: string;
    groupName: string;
    tailscale: string | null;
    lan: string[];
  } | null>(null);
  const [copied, setCopied] = useState(false);

  const handleCreate = async () => {
    if (!serverName.trim()) {
      setError('Give your server a name');
      return;
    }
    setLoading(true);
    setError('');
    try {
      const result = await invoke<{
        groupId: string;
        groupName: string;
        inviteCode: string;
        relayAddr: string;
        addresses: {
          tailscale: string | null;
          lan: string[];
          relayPort: number;
          best: string;
        };
      }>('create_server', { name: serverName.trim() });

      setCreateResult({
        inviteCode: result.inviteCode,
        relayAddr: result.relayAddr,
        groupName: result.groupName,
        tailscale: result.addresses.tailscale,
        lan: result.addresses.lan,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleJoin = async () => {
    if (!inviteCode.trim()) {
      setError('Paste an invite code');
      return;
    }
    setLoading(true);
    setError('');
    try {
      await invoke('join_via_invite', { code: inviteCode.trim() });
      await useAppStore.getState().hydrateFromBackend();
      setScreen('chat');
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleCopy = async (text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleContinue = async () => {
    await useAppStore.getState().hydrateFromBackend();
    setScreen('chat');
  };

  return (
    <div className={styles.container}>
      <div className={styles.card} style={{ width: 440 }}>
        <div className={styles.logo}>
          <Shield size={48} />
        </div>

        {/* ── Choose mode ── */}
        {mode === 'choose' && (
          <>
            <h1 className={styles.title}>Get Started</h1>
            <p className={styles.subtitle}>Create your own server or join a friend's.</p>

            <div className={onbStyles.optionCards}>
              <button className={onbStyles.optionCard} onClick={() => setMode('create')}>
                <div className={onbStyles.optionIcon}><Plus size={24} /></div>
                <div className={onbStyles.optionText}>
                  <strong>Create a Server</strong>
                  <span>Host your own encrypted server. Share an invite code with friends.</span>
                </div>
              </button>

              <button className={onbStyles.optionCard} onClick={() => setMode('join')}>
                <div className={onbStyles.optionIcon}><LogIn size={24} /></div>
                <div className={onbStyles.optionText}>
                  <strong>Join a Server</strong>
                  <span>Have an invite code? Paste it to join an existing server.</span>
                </div>
              </button>
            </div>

            <button className={styles.ghostButton} onClick={() => setScreen('chat')}>
              Skip for now
            </button>
          </>
        )}

        {/* ── Create server ── */}
        {mode === 'create' && !createResult && (
          <>
            <h1 className={styles.title}>Create Your Server</h1>
            <p className={styles.subtitle}>
              This starts an encrypted relay on your machine. Your friends connect directly to you.
            </p>

            <div className={styles.form}>
              <label className={styles.label}>Server Name</label>
              <input
                className={styles.input}
                type="text"
                placeholder="My Server"
                value={serverName}
                onChange={(e) => setServerName(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
                autoFocus
                maxLength={100}
              />

              {error && <div className={styles.error}>{error}</div>}

              <button className={styles.primaryButton} onClick={handleCreate} disabled={loading}>
                {loading ? <Loader2 size={16} className={styles.spinner} /> : <Plus size={16} />}
                Create Server
              </button>
              <button className={styles.ghostButton} onClick={() => { setMode('choose'); setError(''); }}>
                <ArrowLeft size={14} /> Back
              </button>
            </div>
          </>
        )}

        {/* ── Server created — show invite code ── */}
        {mode === 'create' && createResult && (
          <>
            <h1 className={styles.title}>{createResult.groupName}</h1>
            <p className={styles.subtitle}>
              Your server is running! Share this invite code with friends.
            </p>

            <div className={onbStyles.inviteSection}>
              <label className={styles.label}>Invite Code</label>
              <div className={onbStyles.codeBox}>
                <code className={onbStyles.code}>{createResult.inviteCode}</code>
                <button
                  className={onbStyles.copyBtn}
                  onClick={() => handleCopy(createResult.inviteCode)}
                >
                  {copied ? <Check size={16} /> : <Copy size={16} />}
                </button>
              </div>

              <div className={onbStyles.addressInfo}>
                {createResult.tailscale && (
                  <div className={onbStyles.addressRow}>
                    <span className={onbStyles.addrLabel}>Tailscale</span>
                    <span className={onbStyles.addrValue}>{createResult.tailscale}</span>
                  </div>
                )}
                {createResult.lan.length > 0 && (
                  <div className={onbStyles.addressRow}>
                    <span className={onbStyles.addrLabel}>LAN</span>
                    <span className={onbStyles.addrValue}>{createResult.lan[0]}</span>
                  </div>
                )}
              </div>
            </div>

            <button className={styles.primaryButton} onClick={handleContinue}>
              Continue to Server
            </button>
          </>
        )}

        {/* ── Join server ── */}
        {mode === 'join' && (
          <>
            <h1 className={styles.title}>Join a Server</h1>
            <p className={styles.subtitle}>
              Paste the invite code you received from a friend.
            </p>

            <div className={styles.form}>
              <label className={styles.label}>Invite Code</label>
              <input
                className={styles.input}
                type="text"
                placeholder="Paste invite code here"
                value={inviteCode}
                onChange={(e) => setInviteCode(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleJoin()}
                autoFocus
              />

              {error && <div className={styles.error}>{error}</div>}

              <button className={styles.primaryButton} onClick={handleJoin} disabled={loading}>
                {loading ? <Loader2 size={16} className={styles.spinner} /> : <LogIn size={16} />}
                Join Server
              </button>
              <button className={styles.ghostButton} onClick={() => { setMode('choose'); setError(''); }}>
                <ArrowLeft size={14} /> Back
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
