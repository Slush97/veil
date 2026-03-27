import { useState } from 'react';
import { MessageSquare, Plus } from 'lucide-react';
import clsx from 'clsx';
import { Badge } from '../common';
import { useAppStore } from '../../store/appStore';
import styles from './ServerStrip.module.css';

export function ServerStrip() {
  const groups = useAppStore((s) => s.groups);
  const activeGroupId = useAppStore((s) => s.activeGroupId);
  const switchGroup = useAppStore((s) => s.switchGroup);
  const [hoveredId, setHoveredId] = useState<string | null>(null);

  return (
    <div className={styles.strip}>
      {/* Home / DMs button */}
      <div className={clsx(styles.homeButton, !activeGroupId && styles.active)}>
        <MessageSquare size={22} />
      </div>

      <div className={styles.separator} />

      {/* Server icons */}
      {groups.map((group) => {
        const isActive = activeGroupId === group.id;
        const isHovered = hoveredId === group.id;
        const hasUnread = group.unreadCount > 0;

        return (
          <div
            key={group.id}
            className={clsx(styles.serverIcon, isActive && styles.active)}
            onClick={() => switchGroup(group.id)}
            onMouseEnter={() => setHoveredId(group.id)}
            onMouseLeave={() => setHoveredId(null)}
            title={group.name}
          >
            {/* Left pill indicator */}
            {(isActive || isHovered || hasUnread) && (
              <span
                className={clsx(
                  styles.pill,
                  isActive && styles.active,
                  !isActive && isHovered && styles.hover,
                  !isActive && !isHovered && hasUnread && styles.unread,
                )}
              />
            )}

            {/* Initials */}
            {group.name.slice(0, 2)}

            {/* Unread badge */}
            {hasUnread && !isActive && (
              <span className={styles.unreadBadge}>
                <Badge count={group.unreadCount} />
              </span>
            )}
          </div>
        );
      })}

      <div className={styles.separator} />

      {/* Add server */}
      <div className={styles.addButton} title="Create Group">
        <Plus size={22} />
      </div>
    </div>
  );
}
