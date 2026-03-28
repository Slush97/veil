import { useState, useRef, useEffect } from 'react';
import { MessageSquare, Plus, Pencil, Trash2, LogIn } from 'lucide-react';
import clsx from 'clsx';
import { Badge } from '../common';
import { useAppStore } from '../../store/appStore';
import { CreateGroupModal, DeleteGroupModal, JoinServerModal } from '../modals';
import styles from './ServerStrip.module.css';

interface ContextMenu {
  x: number;
  y: number;
  groupId: string;
  groupName: string;
}

export function ServerStrip() {
  const groups = useAppStore((s) => s.groups);
  const activeGroupId = useAppStore((s) => s.activeGroupId);
  const switchGroup = useAppStore((s) => s.switchGroup);
  const renameGroup = useAppStore((s) => s.renameGroup);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showJoinModal, setShowJoinModal] = useState(false);
  const [showAddMenu, setShowAddMenu] = useState(false);
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null);
  const addMenuRef = useRef<HTMLDivElement>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const renameRef = useRef<HTMLInputElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Close add menu on click outside
  useEffect(() => {
    const handler = () => setShowAddMenu(false);
    if (showAddMenu) {
      document.addEventListener('click', handler);
      return () => document.removeEventListener('click', handler);
    }
  }, [showAddMenu]);

  // Close context menu on click outside
  useEffect(() => {
    const handler = () => setContextMenu(null);
    if (contextMenu) {
      document.addEventListener('click', handler);
      return () => document.removeEventListener('click', handler);
    }
  }, [contextMenu]);

  // Focus rename input
  useEffect(() => {
    if (renaming && renameRef.current) {
      renameRef.current.focus();
      renameRef.current.select();
    }
  }, [renaming]);

  const handleContextMenu = (e: React.MouseEvent, groupId: string, groupName: string) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, groupId, groupName });
  };

  const handleRenameSubmit = async () => {
    if (!renaming || !renameValue.trim()) {
      setRenaming(null);
      return;
    }
    try {
      await renameGroup(renaming.id, renameValue.trim());
    } catch { /* error logged in store */ }
    setRenaming(null);
  };

  return (
    <>
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
              onContextMenu={(e) => handleContextMenu(e, group.id, group.name)}
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
        <div className={styles.addButtonWrap}>
          <div
            ref={addMenuRef}
            className={styles.addButton}
            title="Add Server"
            onClick={(e) => {
              e.stopPropagation();
              setShowAddMenu(!showAddMenu);
            }}
          >
            <Plus size={22} />
          </div>
        </div>
      </div>

      {/* Context menu */}
      {contextMenu && (
        <div
          ref={menuRef}
          className={styles.contextMenu}
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClick={(e) => e.stopPropagation()}
        >
          <button
            className={styles.menuItem}
            onClick={() => {
              setRenaming({ id: contextMenu.groupId, name: contextMenu.groupName });
              setRenameValue(contextMenu.groupName);
              setContextMenu(null);
            }}
          >
            <Pencil size={14} /> Rename
          </button>
          <button
            className={clsx(styles.menuItem, styles.menuItemDanger)}
            onClick={() => {
              setDeleteTarget({ id: contextMenu.groupId, name: contextMenu.groupName });
              setContextMenu(null);
            }}
          >
            <Trash2 size={14} /> Delete
          </button>
        </div>
      )}

      {/* Rename inline popover */}
      {renaming && (
        <div className={styles.renameOverlay} onClick={() => setRenaming(null)}>
          <div className={styles.renamePopover} onClick={(e) => e.stopPropagation()}>
            <input
              ref={renameRef}
              className={styles.renameInput}
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleRenameSubmit();
                if (e.key === 'Escape') setRenaming(null);
              }}
              onBlur={handleRenameSubmit}
              maxLength={100}
            />
          </div>
        </div>
      )}

      {/* Modals */}
      {/* Add server menu */}
      {showAddMenu && addMenuRef.current && (() => {
        const rect = addMenuRef.current!.getBoundingClientRect();
        return (
          <div
            className={styles.addMenu}
            style={{ left: rect.right + 8, bottom: window.innerHeight - rect.bottom }}
            onClick={(e) => e.stopPropagation()}
          >
            <button
              className={styles.menuItem}
              onClick={() => { setShowCreateModal(true); setShowAddMenu(false); }}
            >
              <Plus size={14} /> Create Server
            </button>
            <button
              className={styles.menuItem}
              onClick={() => { setShowJoinModal(true); setShowAddMenu(false); }}
            >
              <LogIn size={14} /> Join Server
            </button>
          </div>
        );
      })()}

      {showCreateModal && <CreateGroupModal onClose={() => setShowCreateModal(false)} />}
      {showJoinModal && <JoinServerModal onClose={() => setShowJoinModal(false)} />}
      {deleteTarget && (
        <DeleteGroupModal
          groupId={deleteTarget.id}
          groupName={deleteTarget.name}
          onClose={() => setDeleteTarget(null)}
        />
      )}
    </>
  );
}
