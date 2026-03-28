import { useState, useEffect } from 'react';
import { ChevronDown, ChevronRight, Hash, Volume2, Settings, Plus, Trash2, Copy, Check } from 'lucide-react';
import clsx from 'clsx';
import { Avatar, StatusDot } from '../common';
import { VoiceControls } from '../voice/VoiceControls';
import { CreateChannelModal } from '../modals';
import { useAppStore } from '../../store/appStore';
import { createInvite } from '../../api';
import type { Channel } from '../../types';
import styles from './ChannelSidebar.module.css';

interface ChannelContextMenu {
  x: number;
  y: number;
  channelId: string;
  channelName: string;
}

export function ChannelSidebar() {
  const groups = useAppStore((s) => s.groups);
  const activeGroupId = useAppStore((s) => s.activeGroupId);
  const categories = useAppStore((s) => s.categories);
  const channels = useAppStore((s) => s.channels);
  const activeChannelId = useAppStore((s) => s.activeChannelId);
  const switchChannel = useAppStore((s) => s.switchChannel);
  const deleteChannel = useAppStore((s) => s.deleteChannel);
  const voice = useAppStore((s) => s.voice);
  const joinVoiceChannel = useAppStore((s) => s.joinVoiceChannel);

  const activeGroup = groups.find((g) => g.id === activeGroupId);
  const [collapsedCats, setCollapsedCats] = useState<Set<string>>(new Set());
  const [showCreateChannel, setShowCreateChannel] = useState(false);
  const [contextMenu, setContextMenu] = useState<ChannelContextMenu | null>(null);
  const [showServerInfo, setShowServerInfo] = useState(false);
  const [inviteCode, setInviteCode] = useState<string | null>(null);
  const [inviteCopied, setInviteCopied] = useState(false);

  useEffect(() => {
    const handler = () => setContextMenu(null);
    if (contextMenu) {
      document.addEventListener('click', handler);
      return () => document.removeEventListener('click', handler);
    }
  }, [contextMenu]);

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

  const uncategorizedChannels = channels
    .filter((ch) => ch.categoryId === null)
    .sort((a, b) => a.position - b.position);

  const channelIcon = (kind: Channel['kind']) => {
    switch (kind) {
      case 'voice': return <Volume2 size={16} />;
      default: return <Hash size={16} />;
    }
  };

  const handleChannelContextMenu = (e: React.MouseEvent, ch: Channel) => {
    e.preventDefault();
    if (ch.name === 'general') return;
    setContextMenu({ x: e.clientX, y: e.clientY, channelId: ch.id, channelName: ch.name });
  };

  const handleDeleteChannel = async () => {
    if (!contextMenu) return;
    try {
      await deleteChannel(contextMenu.channelId);
    } catch { /* logged in store */ }
    setContextMenu(null);
  };

  const renderChannel = (ch: Channel) => {
    const isVoice = ch.kind === 'voice';
    const isInThisVoice = isVoice && voice.inRoom && voice.channelName === ch.name;

    return (
      <div key={ch.id}>
        <div
          className={clsx(
            styles.channel,
            !isVoice && activeChannelId === ch.id && styles.active,
            isInThisVoice && styles.active,
            ch.unread && styles.unread,
          )}
          onClick={() => {
            if (isVoice) {
              if (!voice.inRoom) joinVoiceChannel(ch.name);
            } else {
              switchChannel(ch.id);
            }
          }}
          onContextMenu={(e) => handleChannelContextMenu(e, ch)}
        >
          <span className={styles.channelIcon}>{channelIcon(ch.kind)}</span>
          <span className={styles.channelName}>{ch.name}</span>
        </div>
        {isInThisVoice && voice.participants.length > 0 && (
          <div className={styles.voiceParticipants}>
            {voice.participants.map((p) => (
              <div
                key={p.peerId}
                className={clsx(styles.voiceUser, p.isSpeaking && styles.speaking)}
              >
                <Volume2 size={12} />
                <span>{p.displayName.substring(0, 12)}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className={styles.sidebar}>
      {/* Server name header */}
      <div
        className={styles.header}
        onClick={async () => {
          setShowServerInfo(!showServerInfo);
          if (!showServerInfo && !inviteCode && activeGroupId) {
            try {
              const invite = await createInvite(activeGroupId);
              setInviteCode(invite.code);
            } catch {
              // Invite creation not available
            }
          }
        }}
      >
        <span className={styles.serverName}>{activeGroup?.name ?? 'Veil'}</span>
        <ChevronDown
          size={16}
          style={{
            color: 'var(--fg-muted)',
            transform: showServerInfo ? 'rotate(180deg)' : 'rotate(0deg)',
            transition: 'transform 0.15s ease',
          }}
        />
      </div>

      {/* Server info dropdown */}
      {showServerInfo && (
        <div className={styles.serverInfoPanel}>
          {inviteCode ? (
            <>
              <div className={styles.infoLabel}>Invite Code</div>
              <div className={styles.inviteRow}>
                <code className={styles.inviteCode}>{inviteCode}</code>
                <button
                  className={styles.inviteCopy}
                  onClick={(e) => {
                    e.stopPropagation();
                    navigator.clipboard.writeText(inviteCode);
                    setInviteCopied(true);
                    setTimeout(() => setInviteCopied(false), 2000);
                  }}
                >
                  {inviteCopied ? <Check size={14} /> : <Copy size={14} />}
                </button>
              </div>
              <div className={styles.infoHint}>Share this code so friends can join your server</div>
            </>
          ) : (
            <div className={styles.infoHint}>
              Click to generate an invite code
            </div>
          )}
        </div>
      )}

      {/* Channel list */}
      <div className={styles.channelList}>
        {/* Categorized channels */}
        {categories.sort((a, b) => a.position - b.position).map((cat) => {
          const isCollapsed = collapsedCats.has(cat.id);
          const catChannels = channelsByCategory(cat.id);

          return (
            <div key={cat.id}>
              <div className={styles.category}>
                <div className={styles.categoryToggle} onClick={() => toggleCategory(cat.id)}>
                  <ChevronRight
                    size={10}
                    className={clsx(styles.categoryChevron, !isCollapsed && 'rotated')}
                    style={{ transform: isCollapsed ? 'rotate(0deg)' : 'rotate(90deg)' }}
                  />
                  <span className={styles.categoryName}>{cat.name}</span>
                </div>
                <button
                  className={styles.addChannelBtn}
                  title="Create Channel"
                  onClick={() => setShowCreateChannel(true)}
                >
                  <Plus size={14} />
                </button>
              </div>

              {!isCollapsed && catChannels.map(renderChannel)}
            </div>
          );
        })}

        {/* Uncategorized channels */}
        {uncategorizedChannels.length > 0 && categories.length === 0 && (
          <div>
            <div className={styles.category}>
              <div className={styles.categoryToggle}>
                <span className={styles.categoryName}>Channels</span>
              </div>
              <button
                className={styles.addChannelBtn}
                title="Create Channel"
                onClick={() => setShowCreateChannel(true)}
              >
                <Plus size={14} />
              </button>
            </div>
            {uncategorizedChannels.map(renderChannel)}
          </div>
        )}
      </div>

      {/* Voice controls */}
      <VoiceControls />

      {/* User panel */}
      <UserPanel />

      {/* Context menu */}
      {contextMenu && (
        <div
          className={styles.channelContextMenu}
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClick={(e) => e.stopPropagation()}
        >
          <button className={styles.menuItemDanger} onClick={handleDeleteChannel}>
            <Trash2 size={14} /> Delete Channel
          </button>
        </div>
      )}

      {/* Create channel modal */}
      {showCreateChannel && <CreateChannelModal onClose={() => setShowCreateChannel(false)} />}
    </div>
  );
}

function UserPanel() {
  const identity = useAppStore((s) => s.identity);
  const connection = useAppStore((s) => s.connection);
  const toggleSettings = useAppStore((s) => s.toggleSettings);
  const displayName = identity.displayName || identity.username || 'You';
  const statusText = connection.wsState === 'connected' ? 'Online' : 'Connecting...';
  const statusDot = connection.wsState === 'connected' ? 'online' as const : 'idle' as const;

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
        <button className={styles.panelButton} title="Settings" onClick={toggleSettings}>
          <Settings size={18} />
        </button>
      </div>
    </div>
  );
}
