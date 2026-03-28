import { useState, useRef, useEffect, useCallback } from 'react';
import {
  Hash, Pin, Users, Search, Smile, Reply, MoreHorizontal,
  Paperclip, Send, X, Shield, UserPlus, FileText, Download,
  Play, Image as ImageIcon,
} from 'lucide-react';
import clsx from 'clsx';
import { format, isToday, isYesterday } from 'date-fns';
import { open } from '@tauri-apps/plugin-dialog';
import { readImage } from '@tauri-apps/plugin-clipboard-manager';
import { Avatar } from '../common';
import { EmojiGifPicker } from '../chat/EmojiGifPicker';
// @ts-ignore
import emojiData from '@emoji-mart/data';
import { useAppStore } from '../../store/appStore';
import { getFileUrl, getThumbnailUrl, uploadFile } from '../../api';
import type { ChatMessage, MessageKind } from '../../types';
import styles from './ChatArea.module.css';

// Build shortcode lookup from emoji-mart data
const emojiEntries: { id: string; native: string; keywords: string[] }[] = [];
for (const [id, emoji] of Object.entries((emojiData as any).emojis as Record<string, any>)) {
  const native = emoji.skins?.[0]?.native;
  if (native) {
    emojiEntries.push({ id, native, keywords: emoji.keywords || [] });
  }
}

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
  const [showPicker, setShowPicker] = useState(false);
  const [emojiSuggestions, setEmojiSuggestions] = useState<{ id: string; native: string }[]>([]);
  const [selectedSuggestion, setSelectedSuggestion] = useState(0);
  const [attachment, setAttachment] = useState<{
    file: File;
    filename: string;
    previewUrl: string | null;
    sending: boolean;
  } | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages.length]);

  const sendMessage = async () => {
    // Send attachment if pending
    if (attachment && !attachment.sending) {
      setAttachment({ ...attachment, sending: true });
      try {
        await uploadFile(activeChannelId ?? '', attachment.file, attachment.filename);
      } catch (err) {
        console.error('Failed to send attachment:', err);
      }
      setAttachment(null);
      // Also send text if there is any
      const text = input.trim();
      if (text) {
        const replyId = replyingTo?.id;
        storeSendMessage(text, replyId || undefined);
        setInput('');
        setReplyingTo(null);
      }
      if (textareaRef.current) textareaRef.current.style.height = 'auto';
      return;
    }

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

  const insertEmojiSuggestion = (emoji: { id: string; native: string }) => {
    const match = input.match(/:([a-zA-Z0-9_+-]{2,})$/);
    if (match) {
      setInput(input.slice(0, -match[0].length) + emoji.native);
    }
    setEmojiSuggestions([]);
    textareaRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (emojiSuggestions.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedSuggestion((s) => Math.min(s + 1, emojiSuggestions.length - 1));
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedSuggestion((s) => Math.max(s - 1, 0));
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        insertEmojiSuggestion(emojiSuggestions[selectedSuggestion]);
        return;
      }
      if (e.key === 'Escape') {
        setEmojiSuggestions([]);
        return;
      }
    }

    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  };

  const handleInput = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const value = e.target.value;
    setInput(value);
    const el = e.target;
    el.style.height = 'auto';
    el.style.height = el.scrollHeight + 'px';

    const match = value.match(/:([a-zA-Z0-9_+-]{2,})$/);
    if (match) {
      const query = match[1].toLowerCase();
      const matches = emojiEntries
        .filter((e) => e.id.includes(query) || e.keywords.some((k) => k.includes(query)))
        .slice(0, 8);
      setEmojiSuggestions(matches);
      setSelectedSuggestion(0);
    } else {
      setEmojiSuggestions([]);
    }
  };

  const stageAttachment = useCallback((file: File, filename: string, previewUrl: string | null) => {
    setAttachment({ file, filename, previewUrl, sending: false });
  }, []);

  // Handle clipboard paste for images/files
  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    const hasText = e.clipboardData?.getData('text/plain');
    if (hasText) return;

    const items = e.clipboardData?.items;
    if (items) {
      for (const item of items) {
        if (item.kind === 'file' && item.type.startsWith('image/')) {
          e.preventDefault();
          const file = item.getAsFile();
          if (!file) continue;
          const ext = file.type.split('/')[1] || 'png';
          const filename = `paste-${Date.now()}.${ext}`;
          const preview = URL.createObjectURL(file);
          stageAttachment(file, filename, preview);
          return;
        }
      }
    }

    // Fallback: Use Tauri's native clipboard reader (Linux/Wayland)
    e.preventDefault();
    try {
      const img = await readImage();
      const rgba = await img.rgba();
      const size = await img.size();

      if (size.width === 0 || size.height === 0) return;

      const canvas = document.createElement('canvas');
      canvas.width = size.width;
      canvas.height = size.height;
      const ctx = canvas.getContext('2d')!;
      const imageData = new ImageData(
        new Uint8ClampedArray(rgba.buffer),
        size.width,
        size.height,
      );
      ctx.putImageData(imageData, 0, 0);

      const blob = await new Promise<Blob | null>((resolve) =>
        canvas.toBlob(resolve, 'image/png'),
      );
      if (!blob) return;

      const filename = `paste-${Date.now()}.png`;
      const preview = URL.createObjectURL(blob);
      const file = new File([blob], filename, { type: 'image/png' });
      stageAttachment(file, filename, preview);
    } catch {
      // No image in clipboard
    }
  }, [stageAttachment]);

  // Open native file picker
  const handleAttach = async () => {
    try {
      const selected = await open({
        multiple: false,
        title: 'Send File',
        filters: [
          {
            name: 'Media',
            extensions: [
              'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg',
              'mp4', 'mkv', 'avi', 'mov', 'webm', 'flv',
              'mp3', 'ogg', 'wav', 'flac', 'aac', 'opus',
              'pdf', 'txt', 'zip', 'tar', 'gz', 'json', 'csv',
            ],
          },
          { name: 'All Files', extensions: ['*'] },
        ],
      });

      if (selected) {
        // File upload via API (Phase 4 stub — will throw for now)
        try {
          const response = await fetch(selected);
          const blob = await response.blob();
          const filename = selected.split('/').pop() ?? 'file';
          const file = new File([blob], filename);
          await uploadFile(activeChannelId ?? '', file, filename);
        } catch (err) {
          console.error('File upload not yet implemented:', err);
        }
      }
    } catch (err) {
      console.error('File picker failed:', err);
    }
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
                This is the beginning of your conversation.
                <br />
                Start chatting with your server members!
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

      {/* Emoji shortcode suggestions */}
      {emojiSuggestions.length > 0 && (
        <div className={styles.emojiSuggestions}>
          {emojiSuggestions.map((emoji, i) => (
            <button
              key={emoji.id}
              className={clsx(styles.emojiSuggestion, i === selectedSuggestion && styles.emojiSuggestionActive)}
              onClick={() => insertEmojiSuggestion(emoji)}
              onMouseEnter={() => setSelectedSuggestion(i)}
            >
              <span className={styles.emojiChar}>{emoji.native}</span>
              <span className={styles.emojiId}>:{emoji.id}:</span>
            </button>
          ))}
        </div>
      )}

      {/* Attachment preview */}
      {attachment && (
        <div className={styles.attachmentPreview}>
          {attachment.previewUrl ? (
            <img src={attachment.previewUrl} alt="" className={styles.attachmentThumb} />
          ) : (
            <div className={styles.attachmentFileIcon}><FileText size={24} /></div>
          )}
          <span className={styles.attachmentName}>{attachment.filename}</span>
          <button className={styles.attachmentRemove} onClick={() => {
            if (attachment.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
            setAttachment(null);
          }}>
            <X size={14} />
          </button>
        </div>
      )}

      {/* Composer */}
      <div className={styles.composer}>
        <div className={clsx(styles.composerInner, (replyingTo || attachment) && { borderTopLeftRadius: 0, borderTopRightRadius: 0 })}>
          <button className={styles.composerButton} title="Attach file" onClick={handleAttach}>
            <Paperclip size={20} />
          </button>
          <textarea
            ref={textareaRef}
            className={styles.composerInput}
            placeholder={attachment ? 'Add a comment...' : `Message #${activeChannel?.name ?? 'general'}`}
            value={input}
            onChange={handleInput}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            rows={1}
          />
          <div style={{ position: 'relative' }}>
            <button
              className={styles.composerButton}
              title="Emoji & GIFs"
              onClick={() => setShowPicker(!showPicker)}
              style={showPicker ? { color: 'var(--accent)' } : undefined}
            >
              <Smile size={20} />
            </button>
            {showPicker && (
              <EmojiGifPicker
                onEmojiSelect={(emoji) => {
                  setInput((prev) => prev + emoji);
                  textareaRef.current?.focus();
                }}
                onGifSelect={(url) => {
                  storeSendMessage(url);
                  setShowPicker(false);
                }}
                onClose={() => setShowPicker(false)}
              />
            )}
          </div>
          {(input.trim() || attachment) && (
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
      {isGroupStart ? (
        <div className={styles.messageAvatar}>
          <Avatar name={message.senderName} size="lg" />
        </div>
      ) : (
        <div className={styles.messageAvatarSpacer}>
          <span className={styles.inlineTimestamp} title={fullTime}>{timeStr}</span>
        </div>
      )}

      <div className={styles.messageBody}>
        {message.replyTo && (
          <div className={styles.replyContext}>
            <span className={styles.replyBar} />
            <span className={styles.replySender}>{message.replyTo.senderName}</span>
            <span className={styles.replyPreview}>{message.replyTo.preview}</span>
          </div>
        )}

        {isGroupStart && (
          <div className={styles.messageMeta}>
            <span className={clsx(styles.senderName, roleClass)}>{message.senderName}</span>
            <span className={styles.messageTimestamp} title={fullTime}>{timeStr}</span>
            {message.pinned && <span className={styles.pinnedTag}>pinned</span>}
          </div>
        )}

        <MediaContent kind={message.kind} edited={message.edited} />

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

// ── Media content renderer ──

const GIF_URL_RE = /^https:\/\/(media\.tenor\.com|media[0-9]*\.giphy\.com|i\.giphy\.com)\/.*\.(gif|mp4|webp)(\?.*)?$/;

function MediaContent({ kind, edited }: { kind: MessageKind; edited: boolean }) {
  switch (kind.type) {
    case 'text': {
      const text = kind.content.trim();
      if (GIF_URL_RE.test(text)) {
        return (
          <div className={styles.mediaImage} style={{ width: 'auto', height: 'auto', maxWidth: 320 }}>
            <img src={text} alt="GIF" className={styles.mediaImg} style={{ width: '100%', height: 'auto' }} />
          </div>
        );
      }
      return (
        <div className={styles.messageText}>
          {kind.content}
          {edited && <span className={styles.editedTag}>(edited)</span>}
        </div>
      );
    }

    case 'image':
      return <ImageMessage fileId={kind.blobId} width={kind.width} height={kind.height} thumbnailUrl={kind.thumbnailUrl} />;

    case 'video':
      return <VideoMessage fileId={kind.blobId} durationSecs={kind.durationSecs} thumbnailUrl={kind.thumbnailUrl} />;

    case 'audio':
      return <AudioMessage fileId={kind.blobId} durationSecs={kind.durationSecs} waveform={kind.waveform} />;

    case 'file':
      return <FileMessage fileId={kind.blobId} filename={kind.filename} sizeBytes={kind.sizeBytes} />;

    case 'gif':
      return (
        <div className={styles.mediaImage} style={{ width: 'auto', height: 'auto', maxWidth: 320 }}>
          <img src={kind.url} alt="GIF" className={styles.mediaImg} style={{ width: '100%', height: 'auto' }} />
        </div>
      );

    default:
      return null;
  }
}

// ── Image message — now uses direct URL ──

function ImageMessage({ fileId, width, height, thumbnailUrl }: {
  fileId: string; width: number; height: number; thumbnailUrl?: string;
}) {
  const [loaded, setLoaded] = useState(false);
  const fullUrl = getFileUrl(fileId);
  const thumbUrl = thumbnailUrl ?? getThumbnailUrl(fileId);

  const maxW = 400;
  const maxH = 300;
  const scale = Math.min(1, maxW / (width || maxW), maxH / (height || maxH));
  const displayW = Math.round((width || maxW) * scale);
  const displayH = Math.round((height || maxH) * scale);

  return (
    <div className={styles.mediaImage} style={{ width: displayW, height: displayH }}>
      <img
        src={loaded ? fullUrl : thumbUrl}
        alt=""
        className={styles.mediaImg}
        style={!loaded ? { filter: 'blur(2px)' } : undefined}
        onLoad={() => {
          if (!loaded) {
            // Preload full image
            const img = new Image();
            img.onload = () => setLoaded(true);
            img.src = fullUrl;
          }
        }}
        onError={() => setLoaded(true)} // fallback to full url on thumb error
      />
    </div>
  );
}

// ── Video message — now uses direct URL ──

function VideoMessage({ fileId, durationSecs, thumbnailUrl }: {
  fileId: string; durationSecs: number; thumbnailUrl?: string;
}) {
  const [playing, setPlaying] = useState(false);
  const videoUrl = getFileUrl(fileId);

  const formatDuration = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m}:${s.toString().padStart(2, '0')}`;
  };

  if (playing) {
    return (
      <div className={styles.mediaVideo}>
        <video src={videoUrl} controls autoPlay className={styles.mediaVideoEl} />
      </div>
    );
  }

  return (
    <div className={styles.mediaVideo} onClick={() => setPlaying(true)}>
      {thumbnailUrl ? (
        <div className={styles.mediaVideoThumb}>
          <img src={thumbnailUrl} alt="" className={styles.mediaImg} />
          <div className={styles.playOverlay}><Play size={32} /></div>
        </div>
      ) : (
        <div className={styles.mediaPlaceholder} style={{ width: 320, height: 180 }}>
          <Play size={32} />
          <span>{formatDuration(durationSecs)}</span>
        </div>
      )}
    </div>
  );
}

// ── Audio message — now uses direct URL ──

function AudioMessage({ fileId, durationSecs, waveform }: {
  fileId: string; durationSecs: number; waveform: number[];
}) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const audioUrl = getFileUrl(fileId);

  const togglePlay = () => {
    if (playing) {
      audioRef.current?.pause();
    } else {
      audioRef.current?.play();
    }
  };

  const formatDuration = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m}:${s.toString().padStart(2, '0')}`;
  };

  return (
    <div className={styles.mediaAudio}>
      <button className={styles.audioPlayBtn} onClick={togglePlay}>
        {playing ? '||' : <Play size={16} />}
      </button>
      <div className={styles.audioWaveform}>
        {waveform.map((v, i) => (
          <div
            key={i}
            className={styles.waveformBar}
            style={{ height: `${Math.max(4, (v / 255) * 32)}px` }}
          />
        ))}
      </div>
      <span className={styles.audioDuration}>{formatDuration(durationSecs)}</span>
      <audio
        ref={audioRef}
        src={audioUrl}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => setPlaying(false)}
      />
    </div>
  );
}

// ── File message — now uses direct URL ──

function FileMessage({ fileId, filename, sizeBytes }: {
  fileId: string; filename: string; sizeBytes: number;
}) {
  const fileUrl = getFileUrl(fileId);

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <a href={fileUrl} download={filename} className={styles.mediaFile} style={{ textDecoration: 'none', color: 'inherit' }}>
      <div className={styles.fileIcon}><FileText size={32} /></div>
      <div className={styles.fileInfo}>
        <span className={styles.fileName}>{filename}</span>
        <span className={styles.fileSize}>{formatSize(sizeBytes)}</span>
      </div>
      <button className={styles.fileDownload}>
        <Download size={18} />
      </button>
    </a>
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

    if (msg.kind.type === 'system') {
      items.push({ type: 'system', message: msg });
      lastSenderId = '';
      lastTimestamp = 0;
      continue;
    }

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
