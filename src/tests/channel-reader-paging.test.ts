import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import type { StoredMessage } from "../lib/utils/models.ts";
import type { ChannelSummary, ChannelView } from "../lib/utils/wire.ts";

type MessageState = {
  accountGeneration: number;
  messages: StoredMessage[];
  olderExhausted: boolean;
  acceptMessages(rows: StoredMessage[]): void;
  prepareChat(chat: string, limit: number): void;
  loadOlder(chat: string): Promise<void>;
  resetAccount(): void;
};

test("newsletter scroll uses server cursor then local prepend, never phone history", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)), cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/channel-reader-paging", import.meta.url)), ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  let messages: MessageState | undefined;
  try {
    const { MessagesState } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/messages.svelte.ts", import.meta.url)));
    const { channels } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/channels.svelte.ts", import.meta.url)));
    const { session } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/session.svelte.ts", import.meta.url)));
    const { channelsFixture, windowFixture } = await server.ssrLoadModule("/ipc.ts");
    const pager: MessageState = new MessagesState();
    messages = pager;
    const account = "channel-account", chat = "news@newsletter";
    const row = (id: string, timestamp: number) => ({ chat, id, timestamp, sort_order: timestamp, sender: "news@newsletter", text: id } as StoredMessage);
    const localLatest = [row("new-2", 102), row("new-1", 101)];
    const firstOlder = row("old-1", 100), secondOlder = row("old-2", 99);
    windowFixture.archive = [...localLatest];
    windowFixture.phoneRequests = 0;
    channelsFixture.view = { channels: [{ jid: chat, name: "News", description: null, picture_url: null,
      subscriber_count: 1, muted: false, followed: true, favorite: false } as ChannelSummary], synced_at: null } as ChannelView;
    channelsFixture.pages = {
      [chat]: { messages: [firstOlder], next_before: "cursor-1", has_more: true },
      [`${chat}:cursor-1`]: { messages: [secondOlder], next_before: null, has_more: false },
    };
    channelsFixture.calls = []; channelsFixture.failure = ""; channelsFixture.defer = ""; channelsFixture.pending = [];
    session.activeAccount = account;
    session.connected = true;
    pager.prepareChat(chat, 50);
    pager.acceptMessages(localLatest);
    await channels.activate(account, pager.accountGeneration, false);

    await pager.loadOlder(chat);
    assert.deepEqual(pager.messages.map((message: StoredMessage) => message.id), ["new-2", "new-1", "old-1"]);
    assert.equal(pager.olderExhausted, false);
    await pager.loadOlder(chat);
    assert.deepEqual(pager.messages.map((message: StoredMessage) => message.id), ["new-2", "new-1", "old-1", "old-2"]);
    assert.equal(channelsFixture.calls.at(-1)?.args?.before, "cursor-1");
    await pager.loadOlder(chat);
    assert.equal(pager.olderExhausted, true);
    assert.equal(channelsFixture.calls.filter((call: { command: string }) => call.command === "channel_messages").length, 2);
    assert.equal(windowFixture.phoneRequests, 0);
    assert.equal(channelsFixture.calls.some((call: { command: string }) => call.command === "load_older"), false);
    await channels.pageMessages(account, pager.accountGeneration, chat, 50, true);
    assert.equal(channelsFixture.calls.at(-1)?.args?.before, null);

    const offline = "cached@newsletter";
    const cachedRows = [{ ...row("cached-new", 202), chat: offline }, { ...row("cached-old", 201), chat: offline }];
    windowFixture.archive.push(...cachedRows);
    pager.prepareChat(offline, 50);
    pager.acceptMessages([cachedRows[0]]);
    await channels.activate(account, pager.accountGeneration, false);
    const pageCalls = channelsFixture.calls.filter((call: { command: string }) => call.command === "channel_messages").length;
    session.connected = false;
    await pager.loadOlder(offline);
    assert.equal(pager.messages.at(-1)?.id, "cached-old");
    assert.equal(pager.olderExhausted, false);
    assert.equal(channelsFixture.calls.filter((call: { command: string }) => call.command === "channel_messages").length, pageCalls);
    assert.equal(windowFixture.phoneRequests, 0);
    channels.reset();
    session.activeAccount = null;
    session.connected = false;
  } finally {
    messages?.resetAccount();
    await server.close();
  }
});
