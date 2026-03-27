import { Shield } from 'lucide-react';
import styles from './LoadingScreen.module.css';

export function LoadingScreen() {
  return (
    <div className={styles.container}>
      <div className={styles.logo}>
        <Shield size={56} />
      </div>
    </div>
  );
}
