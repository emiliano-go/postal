import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import type { ChannelPage, ChannelSummary, ChannelView } from "../lib/utils/wire.ts";

const root = fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url));
type Fixture = {
  view: ChannelView;
  metadata: Record<string, ChannelSummary>;
  pages: Record<string, ChannelPage>;
  canPost: Record<string, boolean>;
  calls: { command: string; args?: Record<string, unknown> }[];
  failure: string;
  defer: string;
  pending: (() => void)[];
};
const channel = (jid: string, followed = false): ChannelSummary => ({
  jid, name: jid.split("@")[0], description: null, picture_url: null, subscriber_count: 12,
  muted: false, followed, favorite: false,
});

test("channels state loads cache, refreshes, looks up and applies server actions", async () => {
  const server = await createServer({ configFile: root, cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/channels-state", import.meta.url)), ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { ChannelsState } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/channels.svelte.ts", import.meta.url)));
    const { channelsFixture: fixture } = await server.ssrLoadModule("/ipc.ts") as { channelsFixture: Fixture };
    fixture.view = { channels: [channel("one@newsletter", true)], synced_at: 100 } as ChannelView;
    fixture.metadata = { "two@newsletter": channel("two@newsletter"), "Ab_c-123": channel("invite@newsletter") };
    fixture.pages = {
      "one@newsletter": { messages: [], next_before: "older", has_more: true },
      "one@newsletter:older": { messages: [], next_before: "oldest", has_more: false },
    };
    fixture.calls = []; fixture.failure = ""; fixture.defer = ""; fixture.pending = [];
    fixture.canPost = { "one@newsletter": true, "two@newsletter": false };
    const state = new ChannelsState();

    await Promise.all([state.activate("account-a", 1, false), state.activate("account-a", 1, false)]);
    assert.deepEqual(fixture.calls.map((call) => call.command), ["channels"]);
    assert.equal(state.view?.channels[0].jid, "one@newsletter");
    await state.refresh("account-a", 1);
    assert.equal(fixture.calls.at(-1)?.command, "refresh_channels");
    const beforeActivation = fixture.calls.length;
    await Promise.all([state.activate("account-a", 1, true), state.activate("account-a", 1, true)]);
    assert.deepEqual(fixture.calls.slice(beforeActivation).map((call) => call.command), ["channels", "refresh_channels"]);
    assert.equal(state.loading, false);
    await state.lookup("account-a", 1, "two@newsletter");
    assert.equal(state.preview?.jid, "two@newsletter");
    await state.lookup("account-a", 1, "https://www.whatsapp.com/channel/Ab_c-123");
    assert.equal(state.preview?.jid, "invite@newsletter");
    assert.equal(fixture.calls.at(-1)?.args?.jid, "Ab_c-123");
    await state.lookup("account-a", 1, "https://not-whatsapp.example/channel/Ab_c-123");
    assert.equal(state.lookupError, "channels.invalid_target");
    assert.equal(await state.follow("account-a", 1, "two@newsletter"), true);
    assert.equal(state.view?.channels.find((row: ChannelSummary) => row.jid === "two@newsletter")?.followed, true);
    assert.equal(await state.setMuted("account-a", 1, "two@newsletter", true), true);
    assert.equal(state.view?.channels.find((row: ChannelSummary) => row.jid === "two@newsletter")?.muted, true);
    assert.equal(await state.setFavorite("account-a", 1, "two@newsletter", true), true);
    assert.equal(state.view?.channels.find((row: ChannelSummary) => row.jid === "two@newsletter")?.favorite, true);
    assert.equal(await state.unfollow("account-a", 1, "two@newsletter"), true);
    assert.equal(state.view?.channels.find((row: ChannelSummary) => row.jid === "two@newsletter")?.followed, false);
    assert.equal((await state.pageMessages("account-a", 1, "one@newsletter"))?.next_before, "older");
    assert.equal((await state.pageMessages("account-a", 1, "one@newsletter"))?.next_before, "oldest");
    assert.equal(fixture.calls.at(-1)?.args?.before, "older");
    assert.equal((await state.pageMessages("account-a", 1, "one@newsletter", 50, true))?.next_before, "older");
    assert.equal(fixture.calls.at(-1)?.args?.before, null);
    assert.deepEqual(fixture.calls.map((call) => call.command), ["channels", "refresh_channels", "channels", "refresh_channels",
      "channel_metadata", "channel_metadata", "follow_channel", "set_channel_muted", "set_channel_favorite", "unfollow_channel",
      "channel_messages", "channel_messages", "channel_messages"]);
    state.reset();
  } finally { await server.close(); }
});

test("stale account work cannot paint channel metadata", async () => {
  const server = await createServer({ configFile: root, cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/channels-stale", import.meta.url)), ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { ChannelsState } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/channels.svelte.ts", import.meta.url)));
    const { channelsFixture: fixture } = await server.ssrLoadModule("/ipc.ts") as { channelsFixture: Fixture };
    fixture.view = { channels: [channel("new@newsletter", true)], synced_at: 101 } as ChannelView;
    fixture.metadata = { "old@newsletter": channel("old@newsletter") };
    fixture.calls = []; fixture.failure = ""; fixture.defer = ""; fixture.pending = [];
    const state = new ChannelsState();
    await state.activate("account-a", 1, false);
    fixture.defer = "channel_metadata";
    const stale = state.lookup("account-a", 1, "old@newsletter");
    await Promise.resolve();
    assert.equal(fixture.pending.length, 1);
    await state.activate("account-b", 2, false);
    fixture.pending.shift()?.();
    await stale;
    assert.equal(state.preview, null);
    assert.equal(state.view?.channels[0].jid, "new@newsletter");
    state.reset();
  } finally { await server.close(); }
});

test("channel authoring obeys server role, queue order, acknowledgement and current scope", async () => {
  const server = await createServer({ configFile: root, cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/channels-authoring", import.meta.url)), ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { ChannelsState } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/channels.svelte.ts", import.meta.url)));
    const { channelsFixture: fixture } = await server.ssrLoadModule("/ipc.ts") as { channelsFixture: Fixture };
    fixture.view = { channels: [channel("admin@newsletter", true), channel("reader@newsletter", true)], synced_at: 100 } as ChannelView;
    fixture.metadata = {};
    fixture.pages = {};
    fixture.calls = []; fixture.failure = ""; fixture.defer = ""; fixture.pending = [];
    fixture.canPost = { "admin@newsletter": true, "reader@newsletter": false };
    const state = new ChannelsState();
    await state.activate("account-a", 1, false);
    assert.equal(await state.checkCanPost("account-a", 1, "admin@newsletter"), true);
    const sent: string[] = [];
    const enqueue = <T>(task: (signal: AbortSignal) => Promise<T>) => {
      sent.push("queued");
      return task(new AbortController().signal);
    };
    const before = structuredClone(state.view);
    assert.equal(await state.postText("account-a", 1, "admin@newsletter", "post", enqueue), true);
    assert.equal(await state.postMedia("account-a", 1, "admin@newsletter", new File(["image"], "photo.jpg", { type: "image/jpeg" }), "caption", enqueue), true);
    assert.equal(await state.postPoll("account-a", 1, "admin@newsletter", "Question?", ["A", "B"], false, enqueue), true);
    assert.equal(await state.editPost("account-a", 1, "admin@newsletter", "channel-1", "Edited", enqueue), true);
    assert.equal(await state.revokePost("account-a", 1, "admin@newsletter", "channel-1", enqueue), true);
    assert.deepEqual(fixture.calls.filter((call) => call.command.startsWith("channel_post") || call.command === "channel_edit_text" || call.command === "channel_revoke_post")
      .map((call) => call.command), ["channel_post_text", "channel_post_media", "channel_post_poll", "channel_edit_text", "channel_revoke_post"]);
    assert.equal(fixture.calls.find((call) => call.command === "channel_post_media")?.args?.data, "aW1hZ2U=");
    assert.deepEqual(state.view, before);
    assert.equal(sent.length, 5);

    assert.equal(await state.checkCanPost("account-a", 1, "reader@newsletter"), false);
    const authoringCalls = fixture.calls.filter((call) => call.command.startsWith("channel_post")).length;
    assert.equal(await state.postText("account-a", 1, "reader@newsletter", "no", enqueue), false);
    assert.equal(fixture.calls.filter((call) => call.command.startsWith("channel_post")).length, authoringCalls);

    fixture.failure = "channel_post_text";
    assert.equal(await state.checkCanPost("account-a", 1, "admin@newsletter"), true);
    assert.equal(await state.postText("account-a", 1, "admin@newsletter", "failed", enqueue), false);
    assert.equal(state.error?.message, "Operation failed.");
    assert.match(state.error?.diagnostic ?? "", /Synthetic channel_post_text failure/);

    fixture.failure = "";
    let release!: () => void;
    let queuedTask: ((signal: AbortSignal) => Promise<unknown>) | undefined;
    let current = true;
    const deferredEnqueue = <T>(task: (signal: AbortSignal) => Promise<T>) => {
      queuedTask = task as (signal: AbortSignal) => Promise<unknown>;
      return new Promise<T>((resolve, reject) => { release = () => { void task(new AbortController().signal).then(resolve, reject); }; });
    };
    assert.equal(await state.checkCanPost("account-a", 1, "admin@newsletter"), true);
    const callsBeforeStale = fixture.calls.length;
    const stale = state.postText("account-a", 1, "admin@newsletter", "stale", deferredEnqueue, () => current);
    current = false;
    release();
    assert.equal(await stale, false);
    assert.equal(queuedTask !== undefined, true);
    assert.equal(fixture.calls.length, callsBeforeStale);
    state.reset();
  } finally { await server.close(); }
});
