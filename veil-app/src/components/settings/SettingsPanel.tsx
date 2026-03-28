import { useState } from 'react';
import { X, Server, Wifi, WifiOff, Globe } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from '../../store/appStore';
import styles from './SettingsPanel.module.css';

export function SettingsPanel() {
  const relayHosting = useAppStore((s) => s.relayHosting);
  const connection = useAppStore((s) => s.connection);
  const startHostedRelay = useAppStore((s) => s.startHostedRelay);
  const stopHostedRelay = useAppStore((s) => s.stopHostedRelay);
  const toggleSettings = useAppStore((s) => s.toggleSettings);

  const [port, setPort] = useState(4433);
  const [voiceEnabled, setVoiceEnabled] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  // Connect to remote relay
  const [relayAddr, setRelayAddr] = useState('');
  const [connectError, setConnectError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);

  const handleStart = async () => {
    setError(null);
    setStarting(true);
    try {
      await startHostedRelay(port, voiceEnabled);
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  };

  const handleConnect = async () => {
    if (!relayAddr.trim()) return;
    setConnectError(null);
    setConnecting(true);
    try {
      await invoke('connect_relay', { addr: relayAddr.trim() });
    } catch (e) {
      setConnectError(String(e));
    } finally {
      setConnecting(false);
    }
  };

  return (
    <div className={styles.overlay} onClick={toggleSettings}>
      <div className={styles.panel} onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div className={styles.header}>
          <h2 className={styles.title}>Settings</h2>
          <button className={styles.closeBtn} onClick={toggleSettings}>
            <X size={20} />
          </button>
        </div>

        {/* Host Relay Server */}
        <section className={styles.section}>
          <div className={styles.sectionHeader}>
            <Server size={18} />
            <h3>Host Relay Server</h3>
          </div>
          <p className={styles.sectionDesc}>
            Run a relay server so others can connect to your group. Share your address for peers to join.
          </p>

          {relayHosting.active ? (
            <div className={styles.relayActive}>
              <div className={styles.statusRow}>
                <span className={styles.statusDot} />
                <span>Relay running on <strong>{relayHosting.addr}</strong></span>
              </div>
              {relayHosting.voiceEnabled && (
                <div className={styles.statusDetail}>Voice enabled on port {port + 1}</div>
              )}
              <button className={styles.btnDanger} onClick={stopHostedRelay}>
                Stop Relay
              </button>
            </div>
          ) : (
            <div className={styles.relayConfig}>
              <div className={styles.field}>
                <label className={styles.label}>Port</label>
                <input
                  type="number"
                  className={styles.input}
                  value={port}
                  onChange={(e) => setPort(Number(e.target.value))}
                  min={1024}
                  max={65535}
                />
              </div>
              <div className={styles.field}>
                <label className={styles.checkLabel}>
                  <input
                    type="checkbox"
                    checked={voiceEnabled}
                    onChange={(e) => setVoiceEnabled(e.target.checked)}
                  />
                  Enable voice (UDP port {port + 1})
                </label>
              </div>
              {error && <div className={styles.error}>{error}</div>}
              <button
                className={styles.btnPrimary}
                onClick={handleStart}
                disabled={starting}
              >
                {starting ? 'Starting...' : 'Start Relay'}
              </button>
            </div>
          )}
        </section>

        {/* Connect to Remote Relay */}
        <section className={styles.section}>
          <div className={styles.sectionHeader}>
            <Globe size={18} />
            <h3>Connect to Relay</h3>
          </div>
          <p className={styles.sectionDesc}>
            Connect to someone else's relay server. Ask them for their address (e.g. 203.0.113.5:4433).
          </p>
          <div className={styles.connectRow}>
            <input
              type="text"
              className={styles.inputWide}
              placeholder="ip:port (e.g. 203.0.113.5:4433)"
              value={relayAddr}
              onChange={(e) => setRelayAddr(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleConnect()}
            />
            <button
              className={styles.btnPrimary}
              onClick={handleConnect}
              disabled={connecting || !relayAddr.trim()}
            >
              {connecting ? 'Connecting...' : 'Connect'}
            </button>
          </div>
          {connectError && <div className={styles.error}>{connectError}</div>}
        </section>

        {/* Connection Status */}
        <section className={styles.section}>
          <div className={styles.sectionHeader}>
            {connection.relayConnected ? <Wifi size={18} /> : <WifiOff size={18} />}
            <h3>Connection</h3>
          </div>
          <div className={styles.connectionGrid}>
            <span className={styles.connLabel}>Relay</span>
            <span className={connection.relayConnected ? styles.connOk : styles.connOff}>
              {connection.relayConnected ? 'Connected' : 'Disconnected'}
            </span>
            <span className={styles.connLabel}>Peers</span>
            <span>{connection.peerCount}</span>
            <span className={styles.connLabel}>Status</span>
            <span>{connection.state}</span>
          </div>
        </section>
      </div>
    </div>
  );
}
