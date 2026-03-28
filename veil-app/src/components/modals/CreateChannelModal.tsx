import { useState } from 'react';
import { X, Hash, Volume2 } from 'lucide-react';
import clsx from 'clsx';
import { useAppStore } from '../../store/appStore';
import styles from './Modal.module.css';

interface Props {
  onClose: () => void;
}

export function CreateChannelModal({ onClose }: Props) {
  const createChannel = useAppStore((s) => s.createChannel);
  const [name, setName] = useState('');
  const [kind, setKind] = useState<'text' | 'voice'>('text');
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const handleCreate = async () => {
    if (!name.trim()) return;
    setError(null);
    setCreating(true);
    try {
      await createChannel(name.trim(), kind);
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
          <h2 className={styles.title}>Create Channel</h2>
          <button className={styles.closeBtn} onClick={onClose}>
            <X size={20} />
          </button>
        </div>

        <div className={styles.body}>
          <div className={styles.field}>
            <label className={styles.label}>Channel Type</label>
            <div className={styles.kindSelect}>
              <button
                className={clsx(styles.kindOption, kind === 'text' && styles.selected)}
                onClick={() => setKind('text')}
              >
                <Hash size={16} /> Text
              </button>
              <button
                className={clsx(styles.kindOption, kind === 'voice' && styles.selected)}
                onClick={() => setKind('voice')}
              >
                <Volume2 size={16} /> Voice
              </button>
            </div>
          </div>

          <div className={styles.field}>
            <label className={styles.label}>Channel Name</label>
            <input
              className={styles.input}
              type="text"
              placeholder={kind === 'text' ? 'new-channel' : 'Lounge'}
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
              autoFocus
              maxLength={50}
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
            {creating ? 'Creating...' : 'Create Channel'}
          </button>
        </div>
      </div>
    </div>
  );
}
