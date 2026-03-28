import { X, Wifi, WifiOff, LogOut, Server } from 'lucide-react';
import { useAppStore } from '../../store/appStore';
import { getServerUrl, clearAuth, disconnectWs } from '../../api';
import styles from './SettingsPanel.module.css';

export function SettingsPanel() {
  const connection = useAppStore((s) => s.connection);
  const toggleSettings = useAppStore((s) => s.toggleSettings);
  const auth = useAppStore((s) => s.auth);

  const handleLogout = () => {
    disconnectWs();
    clearAuth();
    window.location.reload();
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

        {/* Server info */}
        <section className={styles.section}>
          <div className={styles.sectionHeader}>
            <Server size={18} />
            <h3>Server</h3>
          </div>
          <div className={styles.connectionGrid}>
            <span className={styles.connLabel}>URL</span>
            <span>{getServerUrl() ?? 'Not configured'}</span>
            <span className={styles.connLabel}>User</span>
            <span>{auth.user?.display_name ?? auth.user?.username ?? 'Unknown'}</span>
          </div>
        </section>

        {/* Connection Status */}
        <section className={styles.section}>
          <div className={styles.sectionHeader}>
            {connection.wsState === 'connected' ? <Wifi size={18} /> : <WifiOff size={18} />}
            <h3>Connection</h3>
          </div>
          <div className={styles.connectionGrid}>
            <span className={styles.connLabel}>WebSocket</span>
            <span className={connection.wsState === 'connected' ? styles.connOk : styles.connOff}>
              {connection.wsState}
            </span>
          </div>
        </section>

        {/* Logout */}
        <section className={styles.section}>
          <button className={styles.btnDanger} onClick={handleLogout}>
            <LogOut size={16} /> Logout
          </button>
        </section>
      </div>
    </div>
  );
}
