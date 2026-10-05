import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { parse } from "svelte/compiler";
import { messageRailAction } from "../lib/utils/message-rail.ts";

test("message rail maps navigation and actions without stealing modified or composing keys", () => {
  const event = (key: string, overrides: Partial<Pick<KeyboardEvent, "ctrlKey" | "metaKey" | "altKey" | "defaultPrevented" | "isComposing">> = {}, editable = false, charShortcutsEnabled = true) =>
    messageRailAction({ key, ctrlKey: false, metaKey: false, altKey: false, defaultPrevented: false, isComposing: false, ...overrides }, editable, charShortcutsEnabled);
  assert.deepEqual(["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", "Enter", "r", "S", " "].map((key) => event(key)),
    ["previous", "next", "page-previous", "page-next", "first", "last", "menu", "reply", "star", "select"]);
  assert.equal(event("ArrowDown", {}, true), null);
  assert.equal(event("r", { ctrlKey: true }), null);
  assert.equal(event("s", { altKey: true }), null);
  assert.equal(event("Enter", { isComposing: true }), null);
  assert.equal(event("Spacebar"), "select");
  assert.equal(event("r", {}, false, false), null);
  assert.equal(event("s", {}, false, false), null);
  assert.equal(event("ArrowDown", {}, false, false), "next");
});

test("message rail keeps zero overscan and omits SPA SSR mount hints", () => {
  const ast = parse(readFileSync(new URL("../lib/messages/MessageList.svelte", import.meta.url), "utf8"), { modern: true });
  const wrapper = ast.fragment.nodes.find((node) => node.type === "RegularElement" && node.name === "div");
  assert.ok(wrapper && wrapper.type === "RegularElement");
  const list = wrapper.fragment.nodes.find((node) => node.type === "Component" && node.name === "VList");
  assert.ok(list && list.type === "Component");
  const buffer = list.attributes.find((attribute) => attribute.type === "Attribute" && attribute.name === "bufferSize");
  assert.ok(buffer && buffer.type === "Attribute" && buffer.value !== true);
  const value = Array.isArray(buffer.value) ? buffer.value[0] : buffer.value;
  assert.equal(value.type, "ExpressionTag");
  assert.ok(value.type === "ExpressionTag" && value.expression.type === "Literal");
  assert.equal(value.expression.value, 0);
  assert.equal(list.attributes.some((attribute) => attribute.type === "Attribute" && attribute.name === "ssrCount"), false);
});
