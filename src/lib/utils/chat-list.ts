import type { ChatSummary } from "./models";

export type ChatListGroup = "pinned" | "today" | "yesterday" | "week" | "older";
export type ChatListRow = { chat: ChatSummary; group: ChatListGroup };

function calendarDay(date: Date): number {
  return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 86_400_000;
}

export function chatListRows(
  chats: ChatSummary[],
  now = new Date(),
  frozen: ChatListRow[] = [],
  groupByDate = true,
): ChatListRow[] {
  const today = calendarDay(now);
  const groups: ChatListGroup[] = ["pinned", "today", "yesterday", "week", "older"];
  const rows = chats.map((chat): ChatListRow => {
    const age = today - calendarDay(new Date(chat.last_message_at * 1000));
    const group = chat.pinned ? "pinned" : age <= 0 ? "today"
      : age === 1 ? "yesterday" : age <= 7 ? "week" : "older";
    return { chat, group };
  });
  if (groupByDate) rows.sort((a, b) => groups.indexOf(a.group) - groups.indexOf(b.group));
  if (frozen.length === 0) return rows;
  const freshRows = new Map(rows.map((row) => [row.chat.chat, row]));
  const held: ChatListRow[] = [];
  for (const row of frozen) {
    const fresh = freshRows.get(row.chat.chat);
    if (fresh && fresh.chat.pinned !== (row.group === "pinned")) return rows;
    if (fresh) held.push({ chat: fresh.chat, group: row.group });
  }
  return held;
}

/** Whether every field the list draws reads the same. */
function sameSummary(a: ChatSummary, b: ChatSummary) {
  return (
    a.chat === b.chat &&
    a.display_name === b.display_name &&
    a.last_message_at === b.last_message_at &&
    a.last_text === b.last_text &&
    a.last_from_me === b.last_from_me &&
    a.last_sender_name === b.last_sender_name &&
    a.last_sender === b.last_sender &&
    a.last_media_kind === b.last_media_kind &&
    a.message_count === b.message_count &&
    a.unread_count === b.unread_count &&
    a.mention_count === b.mention_count &&
    a.pinned === b.pinned &&
    a.archived === b.archived &&
    a.muted_until === b.muted_until &&
    a.mute_at_all === b.mute_at_all &&
    a.marked_unread === b.marked_unread
  );
}

/**
 * Reuses the previous summary object whenever nothing about it changed, so a
 * list refresh only re-renders the chats that actually moved or changed.
 */
export function mergeSummaries(previous: ChatSummary[], next: ChatSummary[]): ChatSummary[] {
  const known = new Map(previous.map((c) => [c.chat, c]));
  return next.map((summary) => {
    const old = known.get(summary.chat);
    return old && sameSummary(old, summary) ? old : summary;
  });
}
