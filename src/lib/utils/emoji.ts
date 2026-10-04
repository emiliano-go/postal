export type Emoji = {
  emoji: string;
  label: string;
  /** Without colons, most familiar first: `joy`, `face_with_tears_of_joy`. */
  shortcodes: string[];
  tags: string[];
  group: number;
  skins?: { emoji: string }[];
};

export type SkinTone = "default" | "l1" | "l2" | "l3" | "l4" | "l5";
export const SKIN_TONES: readonly SkinTone[] = ["default", "l1", "l2", "l3", "l4", "l5"];

const SKIN_TONE_MODIFIERS: Record<Exclude<SkinTone, "default">, string> = {
  l1: "\u{1F3FB}",
  l2: "\u{1F3FC}",
  l3: "\u{1F3FD}",
  l4: "\u{1F3FE}",
  l5: "\u{1F3FF}",
};
const SKIN_TONE_KEY = "postal.emojiSkinTone.";
export const SKIN_TONE_CHANGE_EVENT = "postal:emoji-skin-tone-changed";

export type EmojiToken = { query: string; raw: string; start: number; end: number; closed: boolean };

export function emojiTokenAt(text: string, caret: number, selectionEnd = caret): EmojiToken | null {
  if (!Number.isInteger(caret) || caret !== selectionEnd || caret < 0 || caret > text.length) return null;
  const match = /(?:^|\s)(:([a-z0-9_+-]{2,})(:)?)$/i.exec(text.slice(0, caret));
  return match ? { query: match[2].toLowerCase(), raw: match[1], start: caret - match[1].length, end: caret, closed: !!match[3] } : null;
}

export function replaceEmojiToken(text: string, token: EmojiToken, emoji: string): { text: string; caret: number } | null {
  if (text.slice(token.start, token.end) !== token.raw) return null;
  return { text: text.slice(0, token.start) + emoji + text.slice(token.end), caret: token.start + emoji.length };
}

export function exactEmojiForToken(all: Emoji[], token: EmojiToken): string | null {
  return token.closed ? all.find((entry) => entry.shortcodes.includes(token.query))?.emoji ?? null : null;
}

/** Emojibase groups in picker order; 2 is skin-tone components and is left out. */
export const GROUPS: { id: number; label: string; icon: string }[] = [
  { id: 0, label: "Smileys & emotion", icon: "😀" },
  { id: 1, label: "People & body", icon: "👋" },
  { id: 3, label: "Animals & nature", icon: "🐻" },
  { id: 4, label: "Food & drink", icon: "🍔" },
  { id: 5, label: "Travel & places", icon: "✈️" },
  { id: 6, label: "Activities", icon: "⚽" },
  { id: 7, label: "Objects", icon: "💡" },
  { id: 8, label: "Symbols", icon: "❤️" },
  { id: 9, label: "Flags", icon: "🏁" },
];

type Row = {
  hexcode: string;
  label: string;
  unicode: string;
  group?: number;
  tags?: string[];
  skins?: { unicode: string }[];
};
type Codes = Record<string, string | string[]>;

let loading: Promise<Emoji[]> | null = null;
const jsonImportOptions = typeof window === "undefined" && !("env" in import.meta)
  ? { with: { type: "json" as const } }
  : undefined;
const ownersByEmoji = new WeakMap<Emoji[], Map<string, Emoji>>();

function emojiIdentity(emoji: string) {
  return emoji.replace(/\uFE0F/g, "");
}

function owners(all: Emoji[]) {
  let byEmoji = ownersByEmoji.get(all);
  if (!byEmoji) {
    byEmoji = new Map();
    for (const entry of all) {
      byEmoji.set(emojiIdentity(entry.emoji), entry);
      for (const skin of entry.skins ?? []) byEmoji.set(emojiIdentity(skin.emoji), entry);
    }
    ownersByEmoji.set(all, byEmoji);
  }
  return byEmoji;
}

/** The emoji table, loaded on first use so it stays out of the startup bundle. */
export function loadEmojis(): Promise<Emoji[]> {
  loading ??= Promise.all([
    import("emojibase-data/en/compact.json", jsonImportOptions),
    import("emojibase-data/en/shortcodes/github.json", jsonImportOptions),
    import("emojibase-data/en/shortcodes/emojibase.json", jsonImportOptions),
  ]).then(([data, github, emojibase]) => {
    const list = (x: string | string[] | undefined) => (x === undefined ? [] : [x].flat());
    return (data.default as Row[])
      .filter((row) => row.group !== undefined && row.group !== 2)
      .map((row) => ({
        emoji: row.unicode,
        label: row.label,
        shortcodes: [
          ...new Set([
            ...list((github.default as Codes)[row.hexcode]),
            ...list((emojibase.default as Codes)[row.hexcode]),
          ]),
        ],
        tags: row.tags ?? [],
        group: row.group!,
        skins: row.skins?.map(({ unicode }) => ({ emoji: unicode })),
      }));
  });
  return loading;
}

/** Selects an existing Emojibase variant, including complete ZWJ sequences. */
export function applySkinTone(emoji: string, all: Emoji[], tone: SkinTone): string {
  const key = emojiIdentity(emoji);
  const entry = owners(all).get(key);
  if (!entry) return emoji;
  if (tone === "default") return emojiIdentity(entry.emoji) === key ? emoji : entry.emoji;
  const modifier = SKIN_TONE_MODIFIERS[tone];
  return entry.skins?.find((skin) => {
    const modifiers = skin.emoji.match(/[\u{1F3FB}-\u{1F3FF}]/gu) ?? [];
    return modifiers.length > 0 && modifiers.every((found) => found === modifier);
  })?.emoji ?? entry.emoji;
}

export function skinToneStorageKey(account: string | null): string {
  return SKIN_TONE_KEY + JSON.stringify(account ?? "");
}

export function readSkinTone(account: string | null, storage?: Pick<Storage, "getItem">): SkinTone {
  try {
    const saved = (storage ?? (typeof localStorage === "undefined" ? undefined : localStorage))
      ?.getItem(skinToneStorageKey(account));
    return saved && SKIN_TONES.includes(saved as SkinTone) ? saved as SkinTone : "default";
  } catch {
    return "default";
  }
}

export function writeSkinTone(account: string | null, tone: SkinTone, storage?: Pick<Storage, "setItem">) {
  try {
    (storage ?? (typeof localStorage === "undefined" ? undefined : localStorage))
      ?.setItem(skinToneStorageKey(account), tone);
  } catch {}
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(SKIN_TONE_CHANGE_EVENT, { detail: { account, tone } }));
  }
}

/** Best matches for a `:query`: shortcode prefix, then shortcode word, then label and tags. */
export function searchEmojis(all: Emoji[], query: string, limit = 24, recent: readonly string[] = []): Emoji[] {
  const q = query.toLowerCase().replace(/^:|:$/g, "");
  if (!q) return [];
  const rank = (e: Emoji) => {
    if (e.shortcodes.some((c) => c === q)) return 0;
    if (e.shortcodes.some((c) => c.startsWith(q))) return 1;
    if (e.shortcodes.some((c) => c.split(/[_-]/).some((w) => w.startsWith(q)))) return 2;
    if (e.label.toLowerCase().includes(q)) return 3;
    if (e.tags.some((t) => t.startsWith(q))) return 4;
    return 9;
  };
  return all
    .map((e) => ({ e, r: rank(e), recent: recent.indexOf(e.emoji) }))
    .filter((x) => x.r < 9)
    .sort((a, b) => a.r - b.r || (a.recent < 0 ? recent.length : a.recent) - (b.recent < 0 ? recent.length : b.recent))
    .slice(0, limit)
    .map((x) => x.e);
}

const RECENT_KEY = "postal.recentEmoji";

export function recentEmojis(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

export function rememberEmoji(emoji: string) {
  const next = [emoji, ...recentEmojis().filter((e) => e !== emoji)].slice(0, 32);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    // Recents are a convenience; losing them is harmless.
  }
}
