import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applySkinTone,
  loadEmojis,
  readSkinTone,
  skinToneStorageKey,
  writeSkinTone,
  type SkinTone,
} from "../lib/utils/emoji.ts";

const variants: [SkinTone, string][] = [
  ["l1", "👋🏻"],
  ["l2", "👋🏼"],
  ["l3", "👋🏽"],
  ["l4", "👋🏾"],
  ["l5", "👋🏿"],
];

test("all five stored tone variants and default apply from compact emoji data", async () => {
  const emojis = await loadEmojis();
  for (const [tone, expected] of variants) {
    assert.equal(applySkinTone("👋", emojis, tone), expected);
    assert.equal(applySkinTone("👋🏻", emojis, tone), expected);
  }
  assert.equal(applySkinTone("👋🏿", emojis, "default"), "👋");
  assert.equal(applySkinTone("❤️", emojis, "l3"), "❤️");
});

test("quick reactions keep their original default identity and receive each tone", async () => {
  const emojis = await loadEmojis();
  assert.equal(applySkinTone("👍", emojis, "default"), "👍");
  assert.equal(applySkinTone("🙏", emojis, "default"), "🙏");
  assert.deepEqual(
    variants.map(([tone]) => applySkinTone("👍", emojis, tone)),
    ["👍🏻", "👍🏼", "👍🏽", "👍🏾", "👍🏿"],
  );
  assert.deepEqual(
    variants.map(([tone]) => applySkinTone("🙏", emojis, tone)),
    ["🙏🏻", "🙏🏼", "🙏🏽", "🙏🏾", "🙏🏿"],
  );
  assert.equal(applySkinTone("👋🏽", emojis, "default"), "👋");
});

test("same-tone selection reuses complete ZWJ variants from compact data", async () => {
  const emojis = await loadEmojis();
  const couple = emojis.find((emoji) => emoji.label === "couple with heart: man, man");
  assert.ok(couple?.skins?.length);
  const expected = ["👨🏻‍❤️‍👨🏻", "👨🏼‍❤️‍👨🏼", "👨🏽‍❤️‍👨🏽", "👨🏾‍❤️‍👨🏾", "👨🏿‍❤️‍👨🏿"];
  variants.forEach(([tone], index) => {
    assert.equal(applySkinTone(couple.emoji, emojis, tone), expected[index]);
  });
});

test("every bundled tone-capable emoji has all five uniform modifier variants", async () => {
  const emojis = await loadEmojis();
  const toneModifiers: Record<Exclude<SkinTone, "default">, string> = {
    l1: "\u{1F3FB}",
    l2: "\u{1F3FC}",
    l3: "\u{1F3FD}",
    l4: "\u{1F3FE}",
    l5: "\u{1F3FF}",
  };
  const capable = emojis.filter((entry) =>
    entry.skins?.some((skin) => /[\u{1F3FB}-\u{1F3FF}]/u.test(skin.emoji)),
  );
  assert.ok(capable.length > 0);
  for (const entry of capable) {
    for (const [tone, modifier] of Object.entries(toneModifiers) as [Exclude<SkinTone, "default">, string][]) {
      const result = applySkinTone(entry.emoji, emojis, tone);
      const selected = entry.skins?.find((skin) => skin.emoji === result);
      assert.ok(selected, entry.label + " " + tone);
      const modifiers = selected.emoji.match(/[\u{1F3FB}-\u{1F3FF}]/gu) ?? [];
      assert.ok(modifiers.length > 0 && modifiers.every((found) => found === modifier), entry.label + " " + tone);
    }
  }

  for (const label of ["person: blond hair", "man", "woman"]) {
    const entry = emojis.find((emoji) => emoji.label === label);
    assert.ok(entry?.skins?.length, label);
    assert.ok(entry.skins.some((skin) => skin.emoji === applySkinTone(entry.emoji, emojis, "l3")), label);
  }
});

test("skin tone preference persists independently per account", () => {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => { data.set(key, value); },
  };
  writeSkinTone("account-one", "l4", storage);
  writeSkinTone("account-two", "l2", storage);
  assert.equal(readSkinTone("account-one", storage), "l4");
  assert.equal(readSkinTone("account-two", storage), "l2");
  data.set(skinToneStorageKey("account-one"), "invalid");
  assert.equal(readSkinTone("account-one", storage), "default");
});
