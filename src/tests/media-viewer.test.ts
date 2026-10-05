import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";
import { mediaViewerKey, nextSlideshowIndex } from "../lib/utils/media-viewer.ts";

const key = (key: string, overrides: Partial<KeyboardEvent> = {}) => ({
  key, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, defaultPrevented: false, isComposing: false, ...overrides,
});

test("media viewer keeps arrows, adds zoom reset and save shortcuts", () => {
  assert.equal(mediaViewerKey(key("ArrowLeft"), true), "previous");
  assert.equal(mediaViewerKey(key("ArrowRight"), true), "next");
  assert.equal(mediaViewerKey(key("+", { shiftKey: true }), true), "zoom-in");
  assert.equal(mediaViewerKey(key("-"), true), "zoom-out");
  assert.equal(mediaViewerKey(key("0"), true), "zoom-reset");
  assert.equal(mediaViewerKey(key("s"), true), "save");
  assert.equal(mediaViewerKey(key("S", { shiftKey: true }), true), "save");
  assert.equal(mediaViewerKey(key("s"), false), null);
});

test("media viewer ignores modified, handled and composing shortcuts", () => {
  for (const event of [
    key("ArrowLeft", { ctrlKey: true }), key("ArrowRight", { metaKey: true }), key("s", { altKey: true }),
    key("ArrowLeft", { shiftKey: true }), key("0", { defaultPrevented: true }), key("s", { isComposing: true }),
  ]) assert.equal(mediaViewerKey(event, true), null);
});

test("slideshow advances until the last item and never runs with reduced motion", () => {
  const items = [{ kind: "image" }, { kind: "video" }, { kind: "image" }];
  assert.equal(nextSlideshowIndex(items, 0, false), 1);
  assert.equal(nextSlideshowIndex(items, 1, false), 2);
  assert.equal(nextSlideshowIndex(items, 2, false), null);
  assert.equal(nextSlideshowIndex([{ kind: "image" }], 0, false), null);
  assert.equal(nextSlideshowIndex(items, 0, true), null);
  assert.equal(nextSlideshowIndex([{ kind: "image" }, { kind: "audio" }, { kind: "image" }], 0, false), 2);
  assert.equal(nextSlideshowIndex([{ kind: "image" }, { kind: "audio" }], 0, false), null);
});

test("MediaViewer SSR exposes captions and localized actions only when allowed", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/media-viewer", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: MediaViewer } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/media/MediaViewer.svelte", import.meta.url)));
    const { installLocaleProvider, loadCatalog, t } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/i18n/localizer.ts", import.meta.url)));
    const item = { id: "image-1", path: "data:image/svg+xml,%3Csvg/%3E", thumb: null, kind: "image", caption: "Synthetic caption",
      author: "Fixture", avatar: null, timestamp: 1791100000 };
    const callbacks = { items: [item, { ...item, id: "image-2" }], index: 0, onclose() {}, onopen() {}, onmediaaction() {}, onreply() {}, onjump() {} };
    const english = render(MediaViewer, { props: callbacks }).body;
    assert.ok(english.includes("Synthetic caption"));
    assert.ok(english.includes(t("page.menu.copy_image")) && english.includes(t("page.menu.save_image")));
    assert.ok(english.includes(t("content.start_slideshow")) && english.includes(t("content.open_in_default_app")));

    const once = render(MediaViewer, { props: { ...callbacks, onopen: undefined, onmediaaction: undefined } }).body;
    assert.ok(once.includes("Synthetic caption"));
    assert.ok(!once.includes(t("page.menu.copy_image")) && !once.includes(t("page.menu.save_image")));
    assert.ok(!once.includes(t("content.open_in_default_app")));

    const catalog = await loadCatalog("ar"), restore = installLocaleProvider(() => ({ locale: "ar", catalog }));
    try {
      const arabic = render(MediaViewer, { props: callbacks }).body;
      assert.ok(arabic.includes(t("page.menu.copy_image")) && arabic.includes(t("page.menu.save_image")));
      assert.ok(arabic.includes(t("content.start_slideshow")) && arabic.includes("Synthetic caption"));
    } finally { restore(); }
  } finally { await server.close(); }
});
