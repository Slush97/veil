import { useState } from 'react';
import { ChevronDown, ChevronRight, Hash, Volume2, Settings } from 'lucide-react';
import clsx from 'clsx';
import { Avatar, StatusDot } from '../common';
import { useAppStore } from '../../store/appStore';
import type { Channel } from '../../types';
import styles from './ChannelSidebar.module.css';

export function ChannelSidebar() {
  const groups = useAppStore((s) => s.groups);
  const activeGroupId = useAppStore((s) => s.activeGroupId);
  const categories = useAppStore((s) => s.categories);
  const channels = useAppStore((s) => s.channels);
  const activeChannelId = useAppStore((s) => s.activeChannelId);
  const switchChannel = useAppStore((s) => s.switchChannel);

  const activeGroup = groups.find((g) => g.id === activeGroupId);
  const [collapsedCats, setCollapsedCats] = useState<Set<string>>(new Set());

  const toggleCategory = (catId: string) => {
    setCollapsedCats((prev) => {
      const next = new Set(prev);
      if (next.has(catId)) next.delete(catId);
      else next.add(catId);
      return next;
    });
  };

  const channelsByCategory = (catId: string) =>
    channels
      .filter((ch) => ch.categoryId === catId)
      .sort((a, b) => a.position - b.position);

  const channelIcon = (kind: Channel['kind']) => {
    switch (kind) {
      case 'voice': return <Volume2 size={16} />;
      default: return <Hash size={16} />;
    }
  };

  return (
    <div className={styles.sidebar}>
      {/* Server name header */}
      <div className={styles.header}>
        <span className={styles.serverName}>{activeGroup?.name ?? 'Veil'}</span>
        <ChevronDown size={16} style={{ color: 'var(--fg-muted)' }} />
      </div>

      {/* Channel list */}
      <div className={styles.channelList}>
        {categories.sort((a, b) => a.position - b.position).map((cat) => {
          const isCollapsed = collapsedCats.has(cat.id);
          const catChannels = channelsByCategory(cat.id);

          return (
            <div key={cat.id}>
              {/* Category header */}
              <div className={styles.category} onClick={() => toggleCategory(cat.id)}>
                <ChevronRight
                  size={10}
                  className={clsx(styles.categoryChevron, !isCollapsed && 'rotated')}
                  style={{ transform: isCollapsed ? 'rotate(0deg)' : 'rotate(90deg)' }}
                />
                <span className={styles.categoryName}>{cat.name}</span>
              </div>

              {/* Channels */}
              {!isCollapsed && catChannels.map((ch) => (
                <div
                  key={ch.id}
                  className={clsx(
                    styles.channel,
                    activeChannelId === ch.id && styles.active,
                    ch.unread && styles.unread,
                  )}
                  onClick={() => switchChannel(ch.name)}
                >
                  <span className={styles.channelIcon}>{channelIcon(ch.kind)}</span>
                  <span className={styles.channelName}>{ch.name}</span>
                </div>
              ))}
            </div>
          );
        })}
      </div>

      {/* User panel */}
      <UserPanel />
    </div>
  );
}

function UserPanel() {
  const identity = useAppStore((s) => s.identity);
  const connection = useAppStore((s) => s.connection);
  const displayName = identity.displayName || identity.username || 'You';
  const statusText = connection.state === 'connected' ? 'Online' : 'Connecting...';
  const statusDot = connection.state === 'connected' ? 'online' as const : 'idle' as const;

  return (
    <div className={styles.userPanel}>
      <div className={styles.userPanelAvatar}>
        <Avatar name={displayName} size="md" />
        <StatusDot status={statusDot} positioned />
      </div>
      <div className={styles.userInfo}>
        <div className={styles.userName}>{displayName}</div>
        <div className={styles.userStatus}>{statusText}</div>
      </div>
      <div className={styles.panelButtons}>
        <button className={styles.panelButton} title="Settings">
          <Settings size={18} />
        </button>
      </div>
    </div>
  );
}
