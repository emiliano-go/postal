// Small message display helpers shared by the page and the message views.
// Moved out of +page.svelte.
import type { StoredMessage } from "./models";
import { t, formatDate, formatTime as localeTime } from "../i18n/localizer.ts";

/** An SVG file sent as a document, which is drawn in place like a picture. */
export function isSvg(m: StoredMessage) {
  return m.media_kind === "document" && /\.svg$/i.test(m.media_path ?? m.text.split("\n")[0].trim());
}
/** A JID without its device suffix, the form pictures and names are keyed by. */
export function bare(jid: string) {
  return jid.replace(/:\d+(?=@)/, "");
}

export function replyIcon(kind: string | null) {
  if (kind === "audio") return "\u{1F3B5}";
  if (kind === "video" || kind === "round_video" || kind === "gif") return "\u{1F3AC}";
  if (kind === "document") return "\u{1F4C4}";
  if (kind === "sticker") return "\u{1F600}";
  if (kind === "image") return "\u{1F5BC}";
  return "\u{1F4CE}";
}

/** A media message's caption. Uncaptioned media is stored as `[kind]`. */
export function captionOf(message: StoredMessage) {
  if (message.spoiler) return t("message.spoiler_caption");
  const text = message.text.trim();
  return text === `[${message.media_kind}]` ? "" : text;
}

export function isUnavailable(message: Pick<StoredMessage, "system_kind">) {
  return message.system_kind === "UNAVAILABLE_MESSAGE";
}

export function dayKey(ts: number) {
  return new Date(ts * 1000).toDateString();
}

export function dayLabel(ts: number) {
  const day = new Date(ts * 1000);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  // Catalog labels, not Intl relative text: English Intl gives lowercase "today".
  if (day.toDateString() === today.toDateString()) return t("chat.date_today");
  if (day.toDateString() === yesterday.toDateString()) return t("chat.date_yesterday");
  return formatDate(ts, {
    day: "numeric",
    month: "long",
    year: day.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}

export function formatTime(seconds: number) {
  return localeTime(seconds, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * Compact timestamp for list rows: the time today, then "Yesterday", the
 * weekday within the past week, and a short date beyond that.
 */
export function listStamp(seconds: number, now = new Date()) {
  const day = new Date(seconds * 1000);
  const age = Math.round(
    (Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()) - Date.UTC(day.getFullYear(), day.getMonth(), day.getDate())) / 86_400_000,
  );
  if (age <= 0) return formatTime(seconds);
  if (age === 1) return t("chat.date_yesterday");
  if (age < 7) return formatDate(seconds, { weekday: "short" });
  return formatDate(seconds, { dateStyle: "short" });
}

/**
 * What a location share says beyond its position: the stored text without
 * the maps link, or "Location" / "Live location" when nothing is left.
 */
export function locationPreview(kind: "location" | "live_location", text: string) {
  const caption = text.split("\n").filter((line) => !line.startsWith("https://maps.google.com")).join("\n").trim();
  return caption || t(kind === "location" ? "state.location" : "state.live_location");
}

function labels(keys: Record<string, string>): Record<string, string> {
  return Object.defineProperties(Object.create(null), Object.fromEntries(Object.entries(keys)
    .map(([kind, code]) => [kind, { enumerable: true, get: () => t(code) }])));
}

export const MEDIA_LABELS = labels({
  image: "media.photo", video: "media.video", round_video: "media.round_video", gif: "media.gif",
  audio: "media.audio", document: "media.document", sticker: "media.sticker",
});

/** Kinds with a view of their own; anything else is drawn as a card. */
export const DRAWN_KINDS = new Set([
  "image",
  "video",
  "round_video",
  "gif",
  "audio",
  "document",
  "sticker",
  "poll",
  "event",
  "view_once",
]);

export const CARD_LABELS = labels({
  album: "message.album", location: "message.location", live_location: "message.live_location",
  contact: "message.contact", music: "message.music", unknown: "message.unsupported",
});

export const VIEW_ONCE_LABEL = labels({
  image: "media.photo", video: "media.video", round_video: "media.round_video", audio: "media.voice",
  gif: "media.gif", sticker: "media.sticker",
});
