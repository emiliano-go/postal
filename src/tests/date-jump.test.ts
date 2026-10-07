import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";
import { localDayRange } from "../lib/utils/local-day.ts";
import type { StoredMessage } from "../lib/utils/models.ts";

function row(id: string, timestamp: number): StoredMessage {
  return { id, chat: "date@s", timestamp, sort_order: timestamp, text: id } as StoredMessage;
}

test("local day range validates dates and ends at the next local midnight", () => {
  const range = localDayRange("2024-02-29");
  assert.ok(range);
  const start = new Date(range.start * 1000), end = new Date(range.end * 1000);
  assert.equal(start.getFullYear(), 2024);
  assert.equal(start.getMonth(), 1);
  assert.equal(start.getDate(), 29);
  assert.equal(start.getHours(), 0);
  assert.equal(end.getMonth(), 2);
  assert.equal(end.getDate(), 1);
  assert.equal(end.getHours(), 0);
  assert.equal(localDayRange("2023-02-29"), null);
  assert.equal(localDayRange("2024-2-09"), null);
});

test("date seek finds local and remote messages without changing the open window; offline, cancellation and exhaustion stay bounded", async () => {
  const server = await createServer({
    configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/date-jump", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } },
    server: { middlewareMode: true, ws: false },
  });
  let pager: { prepareChat(chat: string, limit?: number): void; acceptMessages(rows: StoredMessage[]): void; resetAccount(): void;
    messages: StoredMessage[]; findMessageOnDate(chat: string, start: number, end: number, signal: AbortSignal, progress: (pages: number) => void): Promise<{ status: string; message?: StoredMessage }>;
    isDateSeekActive(chat: string): boolean; consumeDateSeekHistory(chat: string): boolean } | undefined;
  try {
    const { MessagesState } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/messages.svelte.ts", import.meta.url)));
    const { default: MessageFinder } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/messages/MessageFinder.svelte", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const { session } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/session.svelte.ts", import.meta.url)));
    const { windowFixture, dateJumpFixture } = await server.ssrLoadModule("/ipc.ts");
    const state = new MessagesState();
    pager = state;
    const previous = { archive: windowFixture.archive, account: session.activeAccount, connected: session.connected };
    const open = [row("current", 300)];
    session.activeAccount = "date-account";
    session.connected = true;
    windowFixture.archive = [...open];
    dateJumpFixture.pages = [];
    dateJumpFixture.lookups = [];
    dateJumpFixture.requests = [];
    dateJumpFixture.pending = [];
    dateJumpFixture.pendingMarks = [];
    dateJumpFixture.failure = false;
    dateJumpFixture.deferNext = false;
    state.prepareChat("date@s", 100);
    state.acceptMessages(open);
    const finder = render(MessageFinder, { props: { title: "Synthetic chat", placeholder: "Search this chat", items: [], empty: "No results",
      ondatejump: async () => "offline" as const, onopen() {}, onclose() {} } }).body;
    assert.match(finder, /type="date"/);
    assert.match(finder, /Jump to date/);

    windowFixture.archive.push(row("local", 150));
    windowFixture.archive.push(row("older", 50));
    const local = await state.findMessageOnDate("date@s", 100, 200, new AbortController().signal, () => {});
    assert.equal(local.status, "found");
    assert.equal(local.message?.id, "local");
    assert.deepEqual(state.messages.map((message: StoredMessage) => message.id), ["current"]);
    assert.equal(dateJumpFixture.requests.length, 0);

    windowFixture.archive = [...open];
    session.connected = false;
    const offline = await state.findMessageOnDate("date@s", 100, 200, new AbortController().signal, () => {});
    assert.equal(offline.status, "offline");
    assert.equal(dateJumpFixture.requests.length, 0);

    session.connected = true;
    dateJumpFixture.pages = [[row("remote", 150)]];
    const progress: number[] = [];
    const remote = await state.findMessageOnDate("date@s", 100, 200, new AbortController().signal, (pages: number) => progress.push(pages));
    assert.equal(remote.status, "found");
    assert.equal(remote.message?.id, "remote");
    assert.deepEqual(progress, [1]);
    assert.deepEqual(state.messages.map((message: StoredMessage) => message.id), ["current"]);
    assert.deepEqual(dateJumpFixture.requests.at(-1), { accountId: "date-account", chat: "date@s", count: 50 });
    while (state.consumeDateSeekHistory("date@s")) {}

    windowFixture.archive = [...open];
    dateJumpFixture.pages = [[row("before-date", 50)]];
    const missing = await state.findMessageOnDate("date@s", 100, 200, new AbortController().signal, () => {});
    assert.equal(missing.status, "missing");
    assert.deepEqual(state.messages.map((message: StoredMessage) => message.id), ["current"]);
    while (state.consumeDateSeekHistory("date@s")) {}

    windowFixture.archive = [...open];
    dateJumpFixture.pages = [];
    dateJumpFixture.deferNext = true;
    const controller = new AbortController();
    const pending = state.findMessageOnDate("date@s", 100, 200, controller.signal, () => {});
    const deadline = Date.now() + 2_000;
    while (!dateJumpFixture.pending.length && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(dateJumpFixture.pending.length, 1);
    controller.abort();
    assert.equal((await pending).status, "cancelled");
    assert.equal(state.isDateSeekActive("date@s"), true);
    assert.equal(state.consumeDateSeekHistory("date@s"), true);
    assert.equal(state.isDateSeekActive("date@s"), false);
    dateJumpFixture.pending.shift()?.();
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(state.isDateSeekActive("date@s"), false);
    assert.deepEqual(state.messages.map((message: StoredMessage) => message.id), ["current"]);

    windowFixture.archive = [...open, row("stored", 150)];
    windowFixture.pending = [];
    windowFixture.deferNext = true;
    const displayController = new AbortController();
    const display = state.showStoredMessage("date@s", "stored", displayController.signal);
    const displayDeadline = Date.now() + 2_000;
    while (!windowFixture.pending.length && Date.now() < displayDeadline) await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(windowFixture.pending.length, 1);
    displayController.abort();
    windowFixture.pending.shift()?.();
    assert.equal(await display, false);
    assert.deepEqual(state.messages.map((message: StoredMessage) => message.id), ["current"]);

    dateJumpFixture.deferMarks = true;
    const marksController = new AbortController();
    const pendingMarks = state.showStoredMessage("date@s", "stored", marksController.signal);
    const marksDeadline = Date.now() + 2_000;
    while (!dateJumpFixture.pendingMarks.length && Date.now() < marksDeadline) await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(dateJumpFixture.pendingMarks.length, 1);
    marksController.abort();
    dateJumpFixture.pendingMarks.shift()?.();
    assert.equal(await pendingMarks, false);
    assert.deepEqual(state.messages.map((message: StoredMessage) => message.id), ["current"]);

    windowFixture.archive = previous.archive;
    session.activeAccount = previous.account;
    session.connected = previous.connected;
  } finally {
    pager?.resetAccount();
    await server.close();
  }
});
