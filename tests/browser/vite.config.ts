import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [{
    name: "message-menu-lifecycle-fixture",
    resolveId(id) { if (id === "postal-menu-lifecycle.svelte") return "\0postal-menu-lifecycle.svelte"; },
    load(id) {
      if (id !== "\0postal-menu-lifecycle.svelte") return;
      this.addWatchFile(fileURLToPath(new URL("../../src/routes/+page.svelte", import.meta.url)));
      const source = readFileSync(new URL("../../src/routes/+page.svelte", import.meta.url), "utf8");
      const start = source.search(/\{#(?:if|each) ui\.menu\b/);
      const end = source.indexOf("{#if ui.emojiFor}", start);
      if (start < 0 || end < 0) throw new Error("Production message menu block missing");
      return readFileSync(new URL("./MenuLifecycleHarness.svelte", import.meta.url), "utf8")
        .replace("<!-- production-menu -->", source.slice(start, end));
    },
  }, svelte({ configFile: false })],
  resolve: {
    alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("./ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../../src/lib", import.meta.url)) },
    ],
  },
  server: { host: "127.0.0.1", port: 1432, strictPort: true },
});
