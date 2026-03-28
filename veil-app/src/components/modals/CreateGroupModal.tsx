import { useState } from 'react';
import { X } from 'lucide-react';
import { useAppStore } from '../../store/appStore';
import styles from './Modal.module.css';

interface Props {
  onClose: () => void;
}

export function CreateGroupModal({ onClose }: Props) {
  const createGroup = useAppStore((s) => s.createGroup);
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const handleCreate = async () => {
    if (!name.trim()) return;
    setError(null);
    setCreating(true);
    try {
      await createGroup(name.trim());
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2 className={styles.title}>Create Server</h2>
          <button className={styles.closeBtn} onClick={onClose}>
            <X size={20} />
          </button>
        </div>

        <div className={styles.body}>
          <div className={styles.field}>
            <label className={styles.label}>Server Name</label>
            <input
              className={styles.input}
              type="text"
              placeholder="My Server"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
              autoFocus
              maxLength={100}
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
            onClick={handleCreate}
            disabled={creating || !name.trim()}
          >
            {creating ? 'Creating...' : 'Create'}
          </button>
        </div>
      </div>
    </div>
  );
}
