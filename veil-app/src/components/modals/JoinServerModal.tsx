import { useState } from 'react';
import { X } from 'lucide-react';
import { useAppStore } from '../../store/appStore';
import { acceptInvite } from '../../api';
import styles from './Modal.module.css';

interface Props {
  onClose: () => void;
}

export function JoinServerModal({ onClose }: Props) {
  const [code, setCode] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [joining, setJoining] = useState(false);

  const handleJoin = async () => {
    if (!code.trim()) return;
    setError(null);
    setJoining(true);
    try {
      const invite = await acceptInvite(code.trim());

      // Add group to store
      const state = useAppStore.getState();
      const newGroup = {
        id: invite.server_id,
        name: invite.server_name,
        description: '',
        unreadCount: 0,
      };
      useAppStore.setState({
        groups: [...state.groups, newGroup],
        activeGroupId: invite.server_id,
      });

      // Hydrate messages for the new group
      await useAppStore.getState().hydrateFromBackend();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setJoining(false);
    }
  };

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2 className={styles.title}>Join a Server</h2>
          <button className={styles.closeBtn} onClick={onClose}>
            <X size={20} />
          </button>
        </div>

        <div className={styles.body}>
          <p className={styles.warningText}>
            Paste an invite code from a friend to join their server.
          </p>
          <div className={styles.field}>
            <label className={styles.label}>Invite Code</label>
            <input
              className={styles.input}
              type="text"
              placeholder="Paste invite code here"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleJoin()}
              autoFocus
            />
          </div>
          {error && <div className={styles.error}>{error}</div>}
        </div>

        <div className={styles.footer}>
          <button className={styles.btnSecondary} onClick={onClose}>
            Cancel
          </button>
          <button
            className={styles.btnPrimary}
            onClick={handleJoin}
            disabled={joining || !code.trim()}
          >
            {joining ? 'Joining...' : 'Join Server'}
          </button>
        </div>
      </div>
    </div>
  );
}
