import clsx from 'clsx';
import styles from './StatusDot.module.css';

interface StatusDotProps {
  status: 'online' | 'idle' | 'dnd' | 'offline';
  positioned?: boolean;
}

export function StatusDot({ status, positioned = false }: StatusDotProps) {
  return (
    <span className={clsx(styles.dot, styles[status], positioned && styles.positioned)} />
  );
}
