import clsx from 'clsx';
import styles from './Badge.module.css';

interface BadgeProps {
  count: number;
  variant?: 'accent' | 'red' | 'subtle';
}

export function Badge({ count, variant = 'red' }: BadgeProps) {
  if (count <= 0) return null;
  return (
    <span className={clsx(styles.badge, styles[variant])}>
      {count > 99 ? '99+' : count}
    </span>
  );
}
