import { useState, useRef, useEffect } from 'react';
import {
  Hash, Pin, Users, Search, Smile, Reply, MoreHorizontal,
  Paperclip, Send, X, Shield, UserPlus,
} from 'lucide-react';
import clsx from 'clsx';
import { format, isToday, isYesterday } from 'date-fns';
import { Avatar } from '../common';
import { useAppStore } from '../../store/appStore';
import type { ChatMessage } from '../../types';
import styles from './ChatArea.module.css';

export function ChatArea() {
  const channels = useAppStore((s) => s.channels);
  const activeChannelId = useAppStore((s) => s.activeChannelId);
  const messages = useAppStore((s) => s.messages);
  const members = useAppStore((s) => s.members);
  const storeSendMessage = useAppStore((s) => s.sendMessage);
  const toggleMemberList = useAppStore((s) => s.toggleMemberList);
  const memberListOpen = useAppStore((s) => s.ui.memberListOpen);
  const togglePins = useAppStore((s) => s.togglePins);
  const showPins = useAppStore((s) => s.ui.showPins);
  const replyingTo = useAppStore((s) => s.ui.replyingTo);
  const setReplyingTo = useAppStore((s) => s.setReplyingTo);

  const activeChannel = channels.find((c) => c.id === activeChannelId);
  const typingMembers = members.filter((m) => m.isTyping && !m.isSelf);

  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length]);

  const sendMessage = () => {
    const text = input.trim();
    if (!text) return;

    const replyId = replyingTo?.id;
    storeSendMessage(text, replyId || undefined);

    setInput('');
    setReplyingTo(null);
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  const handleInput = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInput(e.target.value);
    const el = e.target;
    el.style.height = 'auto';
    el.style.height = el.scrollHeight + 'px';
  };

  return (
    <div className={styles.chat}>
      {/* Header */}
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <Hash size={18} className={styles.channelIcon} />
          <span className={styles.channelName}>{activeChannel?.name ?? 'general'}</span>
        </div>
        <div className={styles.headerActions}>
          <button
            className={clsx(styles.headerButton, showPins && styles.active)}
            onClick={togglePins}
            title="Pinned Messages"
          >
            <Pin size={18} />
          </button>
          <button
            className={clsx(styles.headerButton, memberListOpen && styles.active)}
            onClick={toggleMemberList}
            title="Member List"
          >
            <Users size={18} />
          </button>
          <button className={styles.headerButton} title="Search">
            <Search size={18} />
          </button>
        </div>
      </div>

      {/* Messages */}
      <div className={styles.messages}>
        <div className={styles.messagesSpacer} />
        <div className={styles.messagesInner}>
          {messages.length === 0 ? (
            <div className={styles.emptyState}>
              <div className={styles.emptyIconGroup}>
                <Shield size={48} className={styles.emptyIcon} />
              </div>
              <div className={styles.emptyTitle}>Welcome to #{activeChannel?.name ?? 'general'}</div>
              <div className={styles.emptySubtitle}>
                This is the beginning of your encrypted conversation.
                <br />
                Messages are end-to-end encrypted — only group members can read them.
              </div>
              <button className={styles.emptyAction}>
                <UserPlus size={16} />
                Invite Friends
              </button>
            </div>
          ) : (
            <MessageList messages={messages} setReplyingTo={setReplyingTo} />
          )}
          <div ref={messagesEndRef} />
        </div>
      </div>

      {/* Typing indicator */}
      <div className={styles.typingBar}>
        {typingMembers.length > 0 && (
          <>
            <span className={styles.typingDots}>
              <span className={styles.typingDot} />
              <span className={styles.typingDot} />
              <span className={styles.typingDot} />
            </span>
            <strong>{typingMembers.map((m) => m.displayName).join(', ')}</strong>
            {typingMembers.length === 1 ? ' is typing...' : ' are typing...'}
          </>
        )}
      </div>

      {/* Reply preview */}
      {replyingTo && (
        <div className={styles.replyPreviewBar}>
          <Reply size={14} />
          <span>Replying to <strong>{replyingTo.senderName}</strong></span>
          <button className={styles.closeButton} onClick={() => setReplyingTo(null)}>
            <X size={14} />
          </button>
        </div>
      )}

      {/* Composer */}
      <div className={styles.composer}>
        <div className={clsx(styles.composerInner, replyingTo && { borderTopLeftRadius: 0, borderTopRightRadius: 0 })}>
          <button className={styles.composerButton} title="Attach file">
            <Paperclip size={20} />
          </button>
          <textarea
            ref={textareaRef}
            className={styles.composerInput}
            placeholder={`Message #${activeChannel?.name ?? 'general'}`}
            value={input}
            onChange={handleInput}
            onKeyDown={handleKeyDown}
            rows={1}
          />
          <button className={styles.composerButton} title="Emoji">
            <Smile size={20} />
          </button>
          {input.trim() && (
            <button className={styles.composerButton} onClick={sendMessage} title="Send" style={{ color: 'var(--accent)' }}>
              <Send size={20} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Message List ──

function MessageList({
  messages,
  setReplyingTo,
}: {
  messages: ChatMessage[];
  setReplyingTo: (msg: ChatMessage | null) => void;
}) {
  const grouped = groupMessages(messages);

  return (
    <>
      {grouped.map((item) => {
        if (item.type === 'date') {
          return (
            <div key={item.key} className={styles.dateSeparator}>
              <div className={styles.dateLine} />
              <span className={styles.dateText}>{item.label}</span>
              <div className={styles.dateLine} />
            </div>
          );
        }

        if (item.type === 'system') {
          return (
            <div key={item.message.id} className={styles.systemMessage}>
              — {item.message.kind.type === 'system' ? item.message.kind.content : ''} —
            </div>
          );
        }

        return (
          <MessageRow
            key={item.message.id}
            message={item.message}
            isGroupStart={item.isGroupStart}
            onReply={() => setReplyingTo(item.message)}
          />
        );
      })}
    </>
  );
}

// ── Message Row ──

function MessageRow({
  message,
  isGroupStart,
  onReply,
}: {
  message: ChatMessage;
  isGroupStart: boolean;
  onReply: () => void;
}) {
  const timeStr = format(new Date(message.timestamp), 'HH:mm');
  const fullTime = format(new Date(message.timestamp), 'MMMM d, yyyy HH:mm');

  const roleClass = message.senderRole === 'owner' || message.senderRole === 'admin'
    ? styles.roleOwner
    : message.senderRole === 'moderator'
      ? styles.roleModerator
      : '';

  return (
    <div className={clsx(styles.messageRow, isGroupStart && styles.groupStart)}>
      {/* Avatar or timestamp gutter */}
      {isGroupStart ? (
        <div className={styles.messageAvatar}>
          <Avatar name={message.senderName} size="lg" />
        </div>
      ) : (
        <div className={styles.messageAvatarSpacer}>
          <span className={styles.inlineTimestamp} title={fullTime}>{timeStr}</span>
        </div>
      )}

      {/* Message body */}
      <div className={styles.messageBody}>
        {/* Reply context */}
        {message.replyTo && (
          <div className={styles.replyContext}>
            <span className={styles.replyBar} />
            <span className={styles.replySender}>{message.replyTo.senderName}</span>
            <span className={styles.replyPreview}>{message.replyTo.preview}</span>
          </div>
        )}

        {/* Sender + timestamp (first in group) */}
        {isGroupStart && (
          <div className={styles.messageMeta}>
            <span className={clsx(styles.senderName, roleClass)}>{message.senderName}</span>
            <span className={styles.messageTimestamp} title={fullTime}>{timeStr}</span>
            {message.pinned && <span className={styles.pinnedTag}>pinned</span>}
          </div>
        )}

        {/* Content */}
        {message.kind.type === 'text' && (
          <div className={styles.messageText}>
            {message.kind.content}
            {message.edited && <span className={styles.editedTag}>(edited)</span>}
          </div>
        )}

        {/* Reactions */}
        {message.reactions.length > 0 && (
          <div className={styles.reactions}>
            {message.reactions.map((r) => (
              <button
                key={r.emoji}
                className={clsx(styles.reaction, r.reacted && styles.reacted)}
              >
                {r.emoji} <span className={styles.reactionCount}>{r.count}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Hover action toolbar */}
      <div className={styles.messageActions}>
        <button className={styles.actionButton} title="Add Reaction">
          <Smile size={16} />
        </button>
        <button className={styles.actionButton} title="Reply" onClick={onReply}>
          <Reply size={16} />
        </button>
        <button className={styles.actionButton} title="More">
          <MoreHorizontal size={16} />
        </button>
      </div>
    </div>
  );
}

// ── Helpers ──

type GroupedItem =
  | { type: 'date'; key: string; label: string }
  | { type: 'system'; message: ChatMessage }
  | { type: 'message'; message: ChatMessage; isGroupStart: boolean };

function groupMessages(messages: ChatMessage[]): GroupedItem[] {
  const items: GroupedItem[] = [];
  let lastDate = '';
  let lastSenderId = '';
  let lastTimestamp = 0;

  for (const msg of messages) {
    const date = new Date(msg.timestamp);
    const dateKey = format(date, 'yyyy-MM-dd');

    // Date separator
    if (dateKey !== lastDate) {
      const label = isToday(date)
        ? 'Today'
        : isYesterday(date)
          ? 'Yesterday'
          : format(date, 'MMMM d, yyyy');
      items.push({ type: 'date', key: `date-${dateKey}`, label });
      lastDate = dateKey;
      lastSenderId = '';
      lastTimestamp = 0;
    }

    // System messages
    if (msg.kind.type === 'system') {
      items.push({ type: 'system', message: msg });
      lastSenderId = '';
      lastTimestamp = 0;
      continue;
    }

    // Group consecutive messages from the same sender within 5 minutes
    const isGroupStart =
      msg.senderId !== lastSenderId ||
      msg.timestamp - lastTimestamp > 5 * 60 * 1000 ||
      !!msg.replyTo;

    items.push({ type: 'message', message: msg, isGroupStart });
    lastSenderId = msg.senderId;
    lastTimestamp = msg.timestamp;
  }

  return items;
}
