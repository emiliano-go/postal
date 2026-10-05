import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { installLocaleProvider, parseCatalog, t } from "../lib/i18n/localizer.ts";
import {
  dynamicSettingSearchFields,
  localizeSettingSearchFields,
  matchSettingSearch,
  settingsSearchShortcut,
  SETTING_SEARCH_FIELDS,
} from "../lib/utils/settings-search.ts";

const root = join(fileURLToPath(new URL("../..", import.meta.url)));
const lib = join(root, "src", "lib");
const en = JSON.parse(readFileSync(join(lib, "i18n/locales/en.json"), "utf8"));
const ar = JSON.parse(readFileSync(join(lib, "i18n/locales/ar.json"), "utf8"));

function findFiles(path: string): string[] {
  return readdirSync(path, { withFileTypes: true }).flatMap((entry) => {
    const full = join(path, entry.name);
    return entry.isDirectory() ? findFiles(full) : full.endsWith(".svelte") ? [full] : [];
  });
}

function hasKey(messages: Record<string, unknown>, key: string): boolean {
  if (typeof messages[key] === "string") return true;
  let value: unknown = messages;
  for (const part of key.split(".")) {
    if (!value || typeof value !== "object" || !(part in value)) return false;
    value = (value as Record<string, unknown>)[part];
  }
  return typeof value === "string";
}

test("settings search matches every normalized term in titles and descriptions", () => {
  const items = [
    { title: "Retención de mensajes", description: "Guardar historial local" },
    { title: "Message font", description: "Adjust text spacing" },
  ];
  assert.deepEqual(matchSettingSearch(items, "retencion historial"), [items[0]]);
  assert.deepEqual(matchSettingSearch(items, "FONT spacing"), [items[1]]);
  assert.deepEqual(matchSettingSearch(items, "   "), []);
  assert.deepEqual(matchSettingSearch(items, "history"), []);
});

test("settings shortcut accepts only unmodified Ctrl/Cmd+F outside IME composition", () => {
  const event = (overrides: Partial<Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "isComposing" | "defaultPrevented">> = {}) => settingsSearchShortcut({
    key: "f", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false,
    isComposing: false, defaultPrevented: false, ...overrides,
  });
  assert.equal(event(), true);
  assert.equal(event({ ctrlKey: false, metaKey: true }), true);
  assert.equal(event({ shiftKey: true }), false);
  assert.equal(event({ altKey: true }), false);
  assert.equal(event({ isComposing: true }), false);
  assert.equal(event({ defaultPrevented: true }), false);
});

test("localized fields preserve descriptions and omit fields outside visible sections", () => {
  const fields = localizeSettingSearchFields([
    { id: "one", section: "privacy", titleKey: "title", descriptionKey: "description" },
    { id: "two", section: "profile", titleKey: "other" },
  ], (key) => ({ title: "Privacy", description: "Keep messages" })[key] ?? key, ["privacy"]);
  assert.deepEqual(fields.map(({ id, title, description }) => ({ id, title, description })), [
    { id: "one", title: "Privacy", description: "Keep messages" },
  ]);
});

test("registered static settings have localized labels and focus targets", () => {
  const source = findFiles(lib).map((path) => readFileSync(path, "utf8")).join("\n");
  const ids = new Set<string>();
  for (const field of SETTING_SEARCH_FIELDS) {
    assert.ok(!ids.has(field.id), `duplicate setting search id ${field.id}`);
    ids.add(field.id);
    assert.ok(hasKey(en, field.titleKey), `missing en title ${field.titleKey}`);
    assert.ok(hasKey(ar, field.titleKey), `missing ar title ${field.titleKey}`);
    if (field.descriptionKey) {
      assert.ok(hasKey(en, field.descriptionKey), `missing en description ${field.descriptionKey}`);
      assert.ok(hasKey(ar, field.descriptionKey), `missing ar description ${field.descriptionKey}`);
    }
    assert.ok(source.includes(`data-setting-search-id=\"${field.id}\"`), `missing focus target ${field.id}`);
  }
});

test("English and Arabic settings descriptions supply every required parameter", () => {
  const sections = [...new Set(SETTING_SEARCH_FIELDS.map(({ section }) => section))];
  for (const [language, messages] of [["en", en], ["ar", ar]] as const) {
    const catalog = parseCatalog(messages);
    const restore = installLocaleProvider(() => ({ locale: language, catalog }));
    try {
      const unavailable = t("locale.text_unavailable");
      const fields = localizeSettingSearchFields(SETTING_SEARCH_FIELDS, t, sections);
      for (const field of fields) {
        assert.notEqual(field.title, unavailable, `${language} title ${field.id}`);
        assert.notEqual(field.description, unavailable, `${language} description ${field.id}`);
      }
      const languageField = fields.find(({ id }) => id === "appearance-language")!;
      assert.ok(matchSettingSearch(fields, languageField.title).includes(languageField));
    } finally { restore(); }
  }
});

test("dynamic settings use their live metadata and deterministic ids", () => {
  const fields = dynamicSettingSearchFields({
    privacy: [{ category: "last", label: "Last seen" }],
    keybinds: [{ id: "open-settings" }],
    mediaKinds: ["images"],
    themeTokens: [{ key: "accent" }],
  });
  assert.deepEqual(fields.map(({ id, titleKey }) => [id, titleKey]), [
    ["whatsapp-privacy-last", "Last seen"],
    ["keybind-open-settings", "settings.main.keybind.open-settings.label"],
    ["media-download-images", "settings.media_images"],
    ["appearance-token-accent", "settings.token_accent"],
  ]);
});
