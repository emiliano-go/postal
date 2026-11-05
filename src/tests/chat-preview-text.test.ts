import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

test("location previews drop the maps link and name captionless shares", async () => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../lib", import.meta.url)) },
    ] },
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/chat-preview-text", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { chats } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/state/chats.svelte.ts", import.meta.url)));
    const preview = (kind: string, text: string) => chats.previewText({ chat: "1@s.whatsapp.net", last_media_kind: kind, last_text: text });
    assert.equal(preview("location", "https://maps.google.com/?q=1,2"), "Location");
    assert.equal(preview("location", "Cafe\nMain St 1\nhttps://maps.google.com/?q=1,2"), "Cafe Main St 1");
    assert.equal(preview("live_location", ""), "Live location");
    assert.equal(preview("live_location", "On my way"), "On my way");
  } finally {
    await server.close();
  }
});
