import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import type { EventHost } from "../lib/state/events.ts";

test("call history events advance the scoped panel refresh revision", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/call-history-events", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const [{ dispatchServiceEvent }, { callHistory }] = await Promise.all([
      server.ssrLoadModule(fileURLToPath(new URL("../lib/state/events.ts", import.meta.url))),
      server.ssrLoadModule(fileURLToPath(new URL("../lib/state/call-history.svelte.ts", import.meta.url))),
    ]);
    const host: EventHost = { scrollToBottom() {}, anchor: () => null, reveal: () => true, reconnect: async () => {} };
    assert.equal(callHistory.revision, 0);
    await dispatchServiceEvent({ kind: "callHistoryChanged" }, host);
    assert.equal(callHistory.revision, 1);
  } finally { await server.close(); }
});
