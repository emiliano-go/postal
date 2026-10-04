import assert from "node:assert/strict";
import test from "node:test";
import { chatListRows, mergeSummaries } from "../../lib/utils/chat-list.ts";
import { execFileSync } from "node:child_process";
import type { ChatSummary } from "../../lib/utils/models.ts";

const summary = (chat: string, over: Partial<ChatSummary> = {}): ChatSummary => ({
  chat,
  display_name: null,
  last_message_at: 1,
  last_text: "hi",
  last_from_me: false,
  last_sender_name: null,
  last_sender: "1@s",
  last_media_kind: null,
  message_count: 1,
  unread_count: 0,
  mention_count: 0,
  pinned: false,
  archived: false,
  muted_until: 0,
  mute_at_all: false,
  marked_unread: false,
  ...over,
});

test("date groups keep pins first, omit empty groups and preserve row order", () => {
  const now = new Date(2026, 9, 3, 12);
  const at = (days: number) => new Date(2026, 9, 3 - days, 12).getTime() / 1000;
  const chats = [summary("older", { last_message_at: at(8) }),
    summary("today", { last_message_at: at(0) }),
    summary("pin", { pinned: true, last_message_at: at(20) }),
    summary("yesterday", { last_message_at: at(1) }),
    summary("week", { last_message_at: at(7) }),
    summary("today-2", { last_message_at: at(0) })];
  const rows = chatListRows(chats, now);
  assert.deepEqual(rows.map((row) => [row.chat.chat, row.group]), [
    ["pin", "pinned"], ["today", "today"], ["today-2", "today"],
    ["yesterday", "yesterday"], ["week", "week"], ["older", "older"],
  ]);
  assert.deepEqual(chatListRows(chats.filter((chat) => chat.chat === "week"), now)
    .map((row) => row.group), ["week"]);
  assert.deepEqual(chatListRows([], now), []);
  assert.equal(chats[0].chat, "older");
});

test("hover freeze holds row positions and headings while refreshing chat data", () => {
  const now = new Date(2026, 9, 3, 12);
  const old = summary("old", { last_message_at: new Date(2026, 9, 1).getTime() / 1000 });
  const recent = summary("recent", { last_message_at: now.getTime() / 1000 });
  const frozen = chatListRows([old, recent], now);
  const updated = { ...old, last_message_at: now.getTime() / 1000, last_text: "new" };
  const incoming = summary("incoming", { last_message_at: now.getTime() / 1000 });
  const rows = chatListRows([updated, incoming, recent], now, frozen);
  assert.deepEqual(rows.map((row) => [row.chat.chat, row.group]), [
    ["recent", "today"], ["old", "week"],
  ]);
  assert.equal(rows[1].chat, updated);
  assert.equal(chatListRows([updated, recent], now)[0].group, "today");
  assert.deepEqual(chatListRows([recent], now, frozen).map((row) => row.chat.chat), ["recent"]);
  const pin = summary("new-pin", { pinned: true });
  assert.deepEqual(chatListRows([old, recent, pin], now, frozen).map((row) => row.chat.chat),
    ["recent", "old"]);
  assert.equal(chatListRows([old, recent, pin], now)[0].chat, pin);
  assert.equal(chatListRows([{ ...old, pinned: true }, recent], now, frozen)[0].group, "pinned");
});

test("local day grouping handles midnight, DST and opposite UTC offsets", () => {
  const moduleUrl = new URL("../../lib/utils/chat-list.ts", import.meta.url).href;
  const script = `
    import assert from "node:assert/strict";
    import { chatListRows } from ${JSON.stringify(moduleUrl)};
    const group = (at, now) => chatListRows([{ last_message_at: new Date(at).getTime() / 1000 }], new Date(now))[0].group;
    assert.equal(group("2026-03-08T23:59:59", "2026-03-09T00:00:00"), "yesterday");
    assert.equal(group("2026-03-08T00:00:00", "2026-03-09T00:00:00"), "yesterday");
    assert.equal(group("2026-11-01T00:00:00", "2026-11-02T00:00:00"), "yesterday");
    assert.equal(group("2026-03-01T12:00:00", "2026-03-08T12:00:00"), "week");
    assert.equal(group("2026-03-01T12:00:00", "2026-03-09T12:00:00"), "older");
    const expected = process.env.TZ === "America/New_York" ? "today" : "yesterday";
    assert.equal(group("2026-10-03T09:59:59Z", "2026-10-03T10:00:00Z"), expected);
  `;
  for (const timezone of ["America/New_York", "Pacific/Kiritimati", "Pacific/Honolulu"]) {
    execFileSync(process.execPath, ["--experimental-strip-types", "--input-type=module", "-e", script],
      { env: { ...process.env, TZ: timezone } });
  }
});

test("Favorites retain their synced order across pin and date differences", () => {
  const rows = [summary("empty"), summary("pin", { pinned: true }),
    summary("today", { last_message_at: Date.now() / 1000 })];
  assert.deepEqual(chatListRows(rows, new Date(), [], false).map((row) => row.chat), rows);
});

test("unchanged chats keep their object, changed and new ones are replaced", () => {
  const first = summary("a@s");
  const second = summary("b@s");
  const merged = mergeSummaries(
    [first, second],
    [summary("a@s"), summary("b@s", { unread_count: 3 }), summary("c@s")],
  );
  assert.equal(merged[0], first, "an unchanged row keeps its identity");
  assert.notEqual(merged[1], second, "a changed row is a new object");
  assert.equal(merged[1].unread_count, 3);
  assert.equal(merged[2].chat, "c@s");
});
