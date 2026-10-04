import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";
import { dismissHelp, helpDismissed, helpShortcut } from "../lib/utils/help.ts";

const event = (overrides: Partial<KeyboardEvent> = {}) => ({ key: "?", ctrlKey: false, metaKey: false,
  altKey: false, isComposing: false, defaultPrevented: false, ...overrides });

test("help dismissal persists and unavailable storage stays non-fatal", () => {
  const values = new Map<string, string>();
  const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value) };
  assert.equal(helpDismissed(storage), false);
  dismissHelp(storage);
  assert.equal(helpDismissed(storage), true);

  assert.equal(helpDismissed({ getItem: () => { throw new Error("blocked"); } }), false);
  assert.doesNotThrow(() => dismissHelp({ setItem: () => { throw new Error("blocked"); } }));
});

test("help shortcut respects preference, focus, IME, handled events and modifiers", () => {
  assert.equal(helpShortcut(event(), true, false), true);
  assert.equal(helpShortcut(event({ shiftKey: true }), true, false), true);
  for (const args of [
    [event(), false, false], [event(), true, true], [event({ isComposing: true }), true, false],
    [event({ defaultPrevented: true }), true, false], [event({ ctrlKey: true }), true, false],
    [event({ metaKey: true }), true, false], [event({ altKey: true }), true, false],
    [event({ key: "/" }), true, false],
  ] as const) assert.equal(helpShortcut(args[0], args[1], args[2]), false);
});

test("shortcut help renders the live binding and translated English and Arabic copy", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/shortcut-help", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: Page } = await server.ssrLoadModule(fileURLToPath(new URL("../routes/+page.svelte", import.meta.url)));
    const { keybinds } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/utils/keybinds.svelte.ts", import.meta.url)));
    const { installLocaleProvider, loadCatalog, t } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/i18n/localizer.ts", import.meta.url)));

    const previous = keybinds.formatBold;
    keybinds.formatBold = { key: "q", ctrl: false, alt: true, shift: false, meta: false };
    try {
      const english = render(Page).body;
      assert.ok(english.includes(t("help.title")) && english.includes("Help and shortcuts"));
      assert.ok(english.includes("Alt+Q"), english.match(/.{0,80}Alt.{0,80}/)?.[0]);
      assert.ok(english.includes("Keyboard shortcuts"));

      const catalog = await loadCatalog("ar"), restore = installLocaleProvider(() => ({ locale: "ar", catalog }));
      try {
        const arabic = render(Page).body;
        assert.ok(arabic.includes("المساعدة والاختصارات"));
        assert.ok(arabic.includes("اختصارات لوحة المفاتيح"));
        assert.ok(arabic.includes("Alt+Q"), arabic.match(/.{0,80}Alt.{0,80}/)?.[0]);
      } finally { restore(); }
    } finally { keybinds.formatBold = previous; }
  } finally { await server.close(); }
});
