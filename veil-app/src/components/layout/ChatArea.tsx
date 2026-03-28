import { useState, useRef, useEffect, useCallback } from 'react';
import {
  Hash, Pin, Users, Search, Smile, Reply, MoreHorizontal,
  Paperclip, Send, X, Shield, UserPlus, FileText, Download,
  Play, Image as ImageIcon,
} from 'lucide-react';
import clsx from 'clsx';
import { format, isToday, isYesterday } from 'date-fns';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { readImage } from '@tauri-apps/plugin-clipboard-manager';
import { Avatar } from '../common';
import { useAppStore } from '../../store/appStore';
import type { ChatMessage, MessageKind } from '../../types';
import styles from './ChatArea.module.css';

export function ChatArea() {
  const channels = useAppStore((s) => s.channels);
  const activeChannelId = useAppStore((s) => s.activeChannelId);
  const messages = useAppStore((s) => s.messages);
  const members = useAppStore((s) => s.members);
  const storeSendMessage = useAppStore((s) => s.sendMessage);
  const sendFile = useAppStore((s) => s.sendFile);
  const sendFileBytes = useAppStore((s) => s.sendFileBytes);
  const toggleMemberList = useAppStore((s) => s.toggleMemberList);
  const memberListOpen = useAppStore((s) => s.ui.memberListOpen);
  const togglePins = useAppStore((s) => s.togglePins);
  const showPins = useAppStore((s) => s.ui.showPins);
  const replyingTo = useAppStore((s) => s.ui.replyingTo);
  const setReplyingTo = useAppStore((s) => s.setReplyingTo);

  const activeChannel = channels.find((c) => c.id === activeChannelId);
  const typingMembers = members.filter((m) => m.isTyping && !m.isSelf);

  const [input, setInput] = useState('');
  const [attachment, setAttachment] = useState<{
    bytes: number[];
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
        await sendFileBytes(attachment.bytes, attachment.filename);
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

  // Stage an attachment for preview before sending
  const stageAttachment = useCallback((bytes: number[], filename: string, previewUrl: string | null) => {
    setAttachment({ bytes, filename, previewUrl, sending: false });
  }, []);

  // Handle clipboard paste for images/files
  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    // Check if there's text being pasted — let text through normally
    const hasText = e.clipboardData?.getData('text/plain');
    if (hasText) return;

    // Check for file items in clipboardData (works on some platforms)
    const items = e.clipboardData?.items;
    if (items) {
      for (const item of items) {
        if (item.kind === 'file' && item.type.startsWith('image/')) {
          e.preventDefault();
          const file = item.getAsFile();
          if (!file) continue;
          const ext = file.type.split('/')[1] || 'png';
          const filename = `paste-${Date.now()}.${ext}`;
          const arrayBuf = await file.arrayBuffer();
          const bytes = Array.from(new Uint8Array(arrayBuf));
          const preview = URL.createObjectURL(file);
          stageAttachment(bytes, filename, preview);
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
      const arrayBuf = await blob.arrayBuffer();
      const bytes = Array.from(new Uint8Array(arrayBuf));
      stageAttachment(bytes, filename, preview);
    } catch {
      // No image in clipboard
    }
  }, [stageAttachment]);

  // Open native file picker — stages the file for preview
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
        // For file picker, send directly via path (more efficient — no byte copying)
        await sendFile(selected);
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
          <button className={styles.composerButton} title="Emoji">
            <Smile size={20} />
          </button>
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
        <MediaContent kind={message.kind} edited={message.edited} />

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

// ── Media content renderer ──

function MediaContent({ kind, edited }: { kind: MessageKind; edited: boolean }) {
  switch (kind.type) {
    case 'text':
      return (
        <div className={styles.messageText}>
          {kind.content}
          {edited && <span className={styles.editedTag}>(edited)</span>}
        </div>
      );

    case 'image':
      return <ImageMessage blobId={kind.blobId} width={kind.width} height={kind.height} thumbnailUrl={kind.thumbnailUrl} />;

    case 'video':
      return <VideoMessage blobId={kind.blobId} durationSecs={kind.durationSecs} thumbnailUrl={kind.thumbnailUrl} />;

    case 'audio':
      return <AudioMessage blobId={kind.blobId} durationSecs={kind.durationSecs} waveform={kind.waveform} />;

    case 'file':
      return <FileMessage blobId={kind.blobId} filename={kind.filename} sizeBytes={kind.sizeBytes} />;

    default:
      return null;
  }
}

// ── Image message ──

function ImageMessage({ blobId, width, height, thumbnailUrl }: {
  blobId: string; width: number; height: number; thumbnailUrl?: string;
}) {
  const [fullUrl, setFullUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const loadFull = async () => {
    if (fullUrl || loading) return;
    setLoading(true);
    try {
      const b64 = await invoke<string>('get_blob', { blobId });
      setFullUrl(`data:image/png;base64,${b64}`);
    } catch (e) {
      console.error('Failed to load image:', e);
    }
    setLoading(false);
  };

  // Constrain display size
  const maxW = 400;
  const maxH = 300;
  const scale = Math.min(1, maxW / (width || maxW), maxH / (height || maxH));
  const displayW = Math.round((width || maxW) * scale);
  const displayH = Math.round((height || maxH) * scale);

  return (
    <div className={styles.mediaImage} onClick={loadFull} style={{ width: displayW, height: displayH }}>
      {fullUrl ? (
        <img src={fullUrl} alt="" className={styles.mediaImg} />
      ) : thumbnailUrl ? (
        <img src={thumbnailUrl} alt="" className={styles.mediaImg} style={{ filter: 'blur(2px)' }} />
      ) : (
        <div className={styles.mediaPlaceholder}>
          <ImageIcon size={32} />
          {loading && <span>Loading...</span>}
        </div>
      )}
    </div>
  );
}

// ── Video message ──

function VideoMessage({ blobId, durationSecs, thumbnailUrl }: {
  blobId: string; durationSecs: number; thumbnailUrl?: string;
}) {
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const loadVideo = async () => {
    if (videoUrl || loading) return;
    setLoading(true);
    try {
      const b64 = await invoke<string>('get_blob', { blobId });
      setVideoUrl(`data:video/mp4;base64,${b64}`);
    } catch (e) {
      console.error('Failed to load video:', e);
    }
    setLoading(false);
  };

  const formatDuration = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m}:${s.toString().padStart(2, '0')}`;
  };

  if (videoUrl) {
    return (
      <div className={styles.mediaVideo}>
        <video src={videoUrl} controls className={styles.mediaVideoEl} />
      </div>
    );
  }

  return (
    <div className={styles.mediaVideo} onClick={loadVideo}>
      {thumbnailUrl ? (
        <div className={styles.mediaVideoThumb}>
          <img src={thumbnailUrl} alt="" className={styles.mediaImg} />
          <div className={styles.playOverlay}><Play size={32} /></div>
        </div>
      ) : (
        <div className={styles.mediaPlaceholder} style={{ width: 320, height: 180 }}>
          <Play size={32} />
          <span>{loading ? 'Loading...' : formatDuration(durationSecs)}</span>
        </div>
      )}
    </div>
  );
}

// ── Audio message ──

function AudioMessage({ blobId, durationSecs, waveform }: {
  blobId: string; durationSecs: number; waveform: number[];
}) {
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);

  const loadAndPlay = async () => {
    if (!audioUrl) {
      setLoading(true);
      try {
        const b64 = await invoke<string>('get_blob', { blobId });
        const url = `data:audio/mpeg;base64,${b64}`;
        setAudioUrl(url);
        // Play after state update
        setTimeout(() => audioRef.current?.play(), 100);
      } catch (e) {
        console.error('Failed to load audio:', e);
      }
      setLoading(false);
      return;
    }
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
      <button className={styles.audioPlayBtn} onClick={loadAndPlay}>
        {loading ? '...' : playing ? '||' : <Play size={16} />}
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
      {audioUrl && (
        <audio
          ref={audioRef}
          src={audioUrl}
          onPlay={() => setPlaying(true)}
          onPause={() => setPlaying(false)}
          onEnded={() => setPlaying(false)}
        />
      )}
    </div>
  );
}

// ── File message ──

function FileMessage({ blobId, filename, sizeBytes }: {
  blobId: string; filename: string; sizeBytes: number;
}) {
  const [downloading, setDownloading] = useState(false);

  const formatSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  const handleDownload = async () => {
    setDownloading(true);
    try {
      const b64 = await invoke<string>('get_blob', { blobId });
      // Create a download link from base64
      const binary = atob(b64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      const blob = new Blob([bytes]);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      console.error('Failed to download file:', e);
    }
    setDownloading(false);
  };

  return (
    <div className={styles.mediaFile} onClick={handleDownload}>
      <div className={styles.fileIcon}><FileText size={32} /></div>
      <div className={styles.fileInfo}>
        <span className={styles.fileName}>{filename}</span>
        <span className={styles.fileSize}>{formatSize(sizeBytes)}</span>
      </div>
      <button className={styles.fileDownload}>
        {downloading ? '...' : <Download size={18} />}
      </button>
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
