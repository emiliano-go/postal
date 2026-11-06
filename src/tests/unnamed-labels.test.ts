import assert from "node:assert/strict";
import test from "node:test";
import { installLocaleProvider, loadCatalog } from "../lib/i18n/localizer.ts";
import { displayName, mentionLabel, phoneLabel } from "../lib/utils/phone.ts";

test("addresses without a readable name never display their identifier", () => {
  assert.equal(displayName(null, "120363429906000000@g.us"), "Unnamed group");
  assert.equal(displayName("120363429906000000", "120363429906000000@g.us"), "Unnamed group");
  assert.equal(displayName(null, "120363000000000000@newsletter"), "Unnamed channel");
  assert.equal(displayName(null, "1234567890@broadcast"), "Broadcast list");
  assert.equal(displayName(null, "138947158093828@lid"), "Unknown contact");
  assert.equal(displayName("138947158093828", "138947158093828:4@lid"), "Unknown contact");
  // No address is not an unknown person.
  assert.equal(displayName(null, ""), "");
});

test("readable names, numbers and masked numbers are kept", () => {
  assert.equal(displayName("Family", "120363429906000000@g.us"), "Family");
  assert.equal(displayName(null, "59891954564@s.whatsapp.net"), phoneLabel("59891954564"));
  assert.equal(displayName("+598∙∙∙∙∙27", "138947158093828@lid"), "+598∙∙∙∙∙27");
  // A phone address whose country cannot be placed still reads as a number.
  assert.equal(displayName(null, "999123@s.whatsapp.net"), "+999123");
});

test("unnamed labels follow the active locale", async () => {
  const catalog = await loadCatalog("ar");
  const restore = installLocaleProvider(() => ({ locale: "ar", catalog }));
  try {
    assert.equal(displayName(null, "120363429906000000@g.us"), catalog["chat.unnamed_group"]);
    assert.equal(displayName(null, "138947158093828@lid"), catalog["chat.unknown_contact"]);
  } finally {
    restore();
  }
});

test("mentions without contact data use the message's own address list", () => {
  assert.equal(mentionLabel("59891954564", ["59891954564@s.whatsapp.net"]), phoneLabel("59891954564"));
  assert.equal(mentionLabel("138947158093828", ["138947158093828@lid"]), "Unknown contact");
  assert.equal(mentionLabel("138947158093828", []), "Unknown contact");
  assert.equal(mentionLabel("59891954564", undefined), "Unknown contact");
});

test("compact labels drop the number when a WhatsApp name is known", () => {
  const identity = { contact_saved: false, saved_name: null, legacy_name: null, push_name: "Martin", username: null, number: "59899983172", own: false };
  assert.equal(displayName(null, "1@lid", identity), `${phoneLabel("59899983172")} · ~Martin`);
  assert.equal(displayName(null, "1@lid", identity, true), "~Martin");
  // Without a name the number is all there is, compact or not.
  assert.equal(displayName(null, "1@lid", { ...identity, push_name: null }, true), phoneLabel("59899983172"));
  // A saved contact already reads as its saved name.
  assert.equal(displayName(null, "1@lid", { ...identity, contact_saved: true, saved_name: "Martín B" }, true), "Martín B");
});
