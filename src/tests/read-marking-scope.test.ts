import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { visibleReadFrontier } from "../lib/utils/album-timeline.ts";

function marking() {
  const source = readFileSync(new URL("../routes/+page.svelte", import.meta.url), "utf8");
  const script = source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("page.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const fn = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "scheduleReadMarking");
  assert.ok(fn);
  const calls: { command: string; args: { account: string; chat: string; id: string } }[] = [];
  let timer!: () => void, resolve!: (changed: number) => void, reject!: (failure: Error) => void;
  let refreshed = 0, visible = ["399"];
  const context = {
    scroller: {}, visibleBoundary: null,
    session: { activeAccount: "account-a" }, chats: { selectedChat: "synthetic@s.whatsapp.net" },
    messages: { ordered: Array.from({ length: 400 }, (_, index) => ({ id: String(index) })), accountGeneration: 1,
      readMarkTimer: undefined, lastMarkedId: null as string | null, firstUnreadId: "0" as string | null,
      lastUnreadId: "399" as string | null },
    messageList: { visibleReadIds: () => visible }, document: { hasFocus: () => true }, visibleReadFrontier,
    clearTimeout() {}, setTimeout(fn: () => void) { timer = fn; return 1; },
    queueRefreshChats() { refreshed++; },
    invoke(command: string, args: { account: string; chat: string; id: string }) {
      calls.push({ command, args });
      return new Promise<number>((yes, no) => { resolve = yes; reject = no; });
    },
  };
  const compiled = ts.transpileModule(fn.getText(tree), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const schedule = runInNewContext(`${compiled}\nscheduleReadMarking`, context) as () => void;
  return { context, calls, schedule, fire: () => timer(), complete: (changed: number) => resolve(changed),
    fail: () => reject(new Error("Synthetic marking failure")), refreshed: () => refreshed,
    visible: (ids: string[]) => { visible = ids; } };
}

const settled = () => new Promise<void>((resolve) => setImmediate(resolve));

test("production read timer stays account-bound before send and after pending receipt", async () => {
  const before = marking(); before.schedule();
  before.context.session.activeAccount = "account-b";
  before.context.messages.accountGeneration++;
  before.fire(); assert.equal(before.calls.length, 0);

  const pending = marking(); pending.schedule(); pending.fire();
  assert.deepEqual(JSON.parse(JSON.stringify(pending.calls)), [{ command: "mark_read_until",
    args: { account: "account-a", chat: "synthetic@s.whatsapp.net", id: "399" } }]);
  pending.context.session.activeAccount = "account-b";
  pending.context.messages.accountGeneration++;
  pending.context.messages.lastMarkedId = "new-account-marker";
  pending.complete(400); await settled();
  assert.equal(pending.refreshed(), 0);
  assert.equal(pending.context.messages.firstUnreadId, "0");
  assert.equal(pending.context.messages.lastMarkedId, "new-account-marker");
});

test("production marker spans skipped virtual rows, preserves below-fold unread, and retries failures", async () => {
  const part = marking(); part.visible(["190", "199"]); part.schedule(); part.fire();
  assert.equal(part.calls[0].args.id, "199");
  part.complete(200); await settled(); assert.equal(part.context.messages.lastUnreadId, "399");
  const bottom = marking(); bottom.schedule(); bottom.fire(); bottom.fail(); await settled();
  assert.equal(bottom.context.messages.lastMarkedId, null);
  bottom.schedule(); bottom.fire(); bottom.complete(400); await settled();
  assert.equal(bottom.calls.length, 2);
  assert.equal(bottom.context.messages.firstUnreadId, null);
  assert.equal(bottom.context.messages.lastUnreadId, null);
});
