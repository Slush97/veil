import { useState, useRef, useEffect, useCallback } from 'react';
import Picker from '@emoji-mart/react';
// @ts-ignore
import data from '@emoji-mart/data';
import { Search, X } from 'lucide-react';
import styles from './EmojiGifPicker.module.css';

const GIPHY_API_KEY = 'GlVGYHkr3WSBnllca54iNt0yFbjz7L65';
const GIPHY_BASE = 'https://api.giphy.com/v1/gifs';

interface Props {
  onEmojiSelect: (emoji: string) => void;
  onGifSelect: (url: string, previewUrl: string) => void;
  onClose: () => void;
}

interface GiphyGif {
  id: string;
  title: string;
  images: {
    original: { url: string; width: string; height: string };
    fixed_width: { url: string; width: string; height: string };
    fixed_width_small: { url: string; width: string; height: string };
    preview_gif: { url: string };
  };
}

export function EmojiGifPicker({ onEmojiSelect, onGifSelect, onClose }: Props) {
  const [tab, setTab] = useState<'emoji' | 'gif'>('emoji');

  return (
    <div className={styles.picker}>
      <div className={styles.tabs}>
        <button
          className={`${styles.tab} ${tab === 'emoji' ? styles.activeTab : ''}`}
          onClick={() => setTab('emoji')}
        >
          Emoji
        </button>
        <button
          className={`${styles.tab} ${tab === 'gif' ? styles.activeTab : ''}`}
          onClick={() => setTab('gif')}
        >
          GIFs
        </button>
        <button className={styles.closeBtn} onClick={onClose}>
          <X size={16} />
        </button>
      </div>

      {tab === 'emoji' ? (
        <div className={styles.emojiPane}>
          <Picker
            data={data}
            onEmojiSelect={(emoji: any) => onEmojiSelect(emoji.native)}
            theme="dark"
            set="native"
            previewPosition="none"
            skinTonePosition="search"
            perLine={8}
            emojiSize={22}
            emojiButtonSize={32}
          />
        </div>
      ) : (
        <GifPane onSelect={onGifSelect} />
      )}
    </div>
  );
}

function GifPane({ onSelect }: { onSelect: (url: string, previewUrl: string) => void }) {
  const [query, setQuery] = useState('');
  const [gifs, setGifs] = useState<GiphyGif[]>([]);
  const [loading, setLoading] = useState(false);
  const searchTimeout = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    fetchTrending();
  }, []);

  const fetchTrending = async () => {
    setLoading(true);
    try {
      const resp = await fetch(
        `${GIPHY_BASE}/trending?api_key=${GIPHY_API_KEY}&limit=24&rating=g`,
      );
      const data = await resp.json();
      setGifs(data.data || []);
    } catch { /* offline */ }
    setLoading(false);
  };

  const searchGifs = useCallback(async (q: string) => {
    if (!q.trim()) {
      fetchTrending();
      return;
    }
    setLoading(true);
    try {
      const resp = await fetch(
        `${GIPHY_BASE}/search?api_key=${GIPHY_API_KEY}&q=${encodeURIComponent(q)}&limit=24&rating=g`,
      );
      const data = await resp.json();
      setGifs(data.data || []);
    } catch {
      setGifs([]);
    }
    setLoading(false);
  }, []);

  const handleInput = (value: string) => {
    setQuery(value);
    if (searchTimeout.current) clearTimeout(searchTimeout.current);
    searchTimeout.current = setTimeout(() => searchGifs(value), 400);
  };

  return (
    <div className={styles.gifPane}>
      <div className={styles.gifSearch}>
        <Search size={16} />
        <input
          className={styles.gifInput}
          type="text"
          placeholder="Search GIFs"
          value={query}
          onChange={(e) => handleInput(e.target.value)}
          autoFocus
        />
      </div>

      <div className={styles.gifGrid}>
        {gifs.map((gif) => (
          <button
            key={gif.id}
            className={styles.gifItem}
            onClick={() => onSelect(
              gif.images.original.url,
              gif.images.fixed_width_small.url,
            )}
            title={gif.title}
          >
            <img
              src={gif.images.fixed_width_small.url}
              alt={gif.title}
              loading="lazy"
            />
          </button>
        ))}
        {loading && <div className={styles.gifLoading}>Loading...</div>}
        {!loading && gifs.length === 0 && query.trim() && (
          <div className={styles.gifEmpty}>No GIFs found</div>
        )}
      </div>

      <div className={styles.tenorBrand}>
        Powered by GIPHY
      </div>
    </div>
  );
}
