import { useState } from 'react';
import { X } from 'lucide-react';
import { useAppStore } from '../../store/appStore';
import styles from './Modal.module.css';

interface Props {
  groupId: string;
  groupName: string;
  onClose: () => void;
}

export function DeleteGroupModal({ groupId, groupName, onClose }: Props) {
  const deleteGroup = useAppStore((s) => s.deleteGroup);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  const handleDelete = async () => {
    setError(null);
    setDeleting(true);
    try {
      await deleteGroup(groupId);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.modal} onClick={(e) => e.stopPropagation()}>
        <div className={styles.header}>
          <h2 className={styles.title}>Delete Server</h2>
          <button className={styles.closeBtn} onClick={onClose}>
            <X size={20} />
          </button>
        </div>

        <div className={styles.body}>
          <p className={styles.warningText}>
            Are you sure you want to delete <strong>{groupName}</strong>? This will remove the server
            and all its channels from your client. This action cannot be undone.
          </p>
          {error && <div className={styles.error}>{error}</div>}
        </div>

        <div className={styles.footer}>
          <button className={styles.btnSecondary} onClick={onClose}>
            Cancel
          </button>
          <button
            className={styles.btnDanger}
            onClick={handleDelete}
            disabled={deleting}
          >
            {deleting ? 'Deleting...' : 'Delete Server'}
          </button>
        </div>
      </div>
    </div>
  );
}
