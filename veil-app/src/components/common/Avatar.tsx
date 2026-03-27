import clsx from 'clsx';
import styles from './Avatar.module.css';

interface AvatarProps {
  name: string;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  className?: string;
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/);
  if (parts.length >= 2) return parts[0][0] + parts[1][0];
  return name.slice(0, 2);
}

export function Avatar({ name, size = 'md', className }: AvatarProps) {
  return (
    <div className={clsx(styles.avatar, styles[size], className)}>
      {initials(name)}
    </div>
  );
}
