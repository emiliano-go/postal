export function pinnedMessageIds(marks: { pinned: string | null; pinned_messages?: readonly string[] }): string[] {
  return Array.isArray(marks.pinned_messages) ? [...marks.pinned_messages] : marks.pinned ? [marks.pinned] : [];
}

export function stepPinnedMessageId(ids: readonly string[], current: string | null, direction: -1 | 1): string | null {
  if (!ids.length) return null;
  const index = ids.indexOf(current ?? "");
  return ids[(Math.max(0, index) + direction + ids.length) % ids.length];
}
