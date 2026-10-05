import assert from "node:assert/strict";
import test from "node:test";
import { pinnedMessageIds, stepPinnedMessageId } from "../lib/utils/message-pins.ts";

test("pinned message ids preserve backend order and only fall back for legacy marks", () => {
  assert.deepEqual(pinnedMessageIds({ pinned: "legacy", pinned_messages: ["new", "old"] }), ["new", "old"]);
  assert.deepEqual(pinnedMessageIds({ pinned: "legacy" }), ["legacy"]);
  assert.deepEqual(pinnedMessageIds({ pinned: "legacy", pinned_messages: [] }), []);
  assert.deepEqual(pinnedMessageIds({ pinned: null, pinned_messages: [] }), []);
});

test("pin stack steps in either direction and wraps without changing order", () => {
  const ids = ["newest", "middle", "oldest"];
  assert.equal(stepPinnedMessageId(ids, "newest", 1), "middle");
  assert.equal(stepPinnedMessageId(ids, "oldest", -1), "middle");
  assert.equal(stepPinnedMessageId(ids, "newest", -1), "oldest");
  assert.equal(stepPinnedMessageId(ids, "oldest", 1), "newest");
  assert.equal(stepPinnedMessageId([], null, 1), null);
});
