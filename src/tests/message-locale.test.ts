import assert from "node:assert/strict";
import test from "node:test";
import arabic from "../lib/i18n/locales/ar.json" with { type: "json" };
import { englishCatalog, installLocaleProvider, t } from "../lib/i18n/localizer.ts";
import { CARD_LABELS, MEDIA_LABELS, VIEW_ONCE_LABEL, captionOf, dayKey, dayLabel, formatTime, listStamp } from "../lib/utils/message.ts";
import { floatContent } from "../lib/utils/float-chat.ts";
import type { StoredMessage } from "../lib/utils/wire.ts";

test("shared media labels and float privacy labels follow locale without changing stored text", () => {
  let state = { locale: "en", catalog: englishCatalog };
  const restore = installLocaleProvider(() => state);
  try {
    const source = { text: "PRIVATE USER TEXT", spoiler: true, media_kind: "image" } as StoredMessage;
    const key = dayKey(123);
    assert.equal(MEDIA_LABELS.image, "Photo");
    state = { locale: "ar", catalog: arabic };
    assert.equal(MEDIA_LABELS.image, t("media.photo"));
    assert.equal(CARD_LABELS.contact, t("message.contact"));
    assert.equal(VIEW_ONCE_LABEL.audio, t("media.voice"));
    assert.equal(captionOf(source), t("message.spoiler_caption"));
    assert.equal(floatContent(source).text, t("message.spoiler"));
    assert.equal(source.text, "PRIVATE USER TEXT");
    assert.equal(dayKey(123), key);
    assert.equal(MEDIA_LABELS.constructor, undefined);
  } finally { restore(); }
});

test("shared dates and times use selected Intl locale rather than host default", () => {
  const restore = installLocaleProvider(() => ({ locale: "ar", catalog: arabic }));
  try {
    assert.equal(formatTime(0), new Intl.DateTimeFormat("ar", { hour: "2-digit", minute: "2-digit" }).format(new Date(0)));
    assert.equal(dayLabel(Date.now() / 1000), arabic["chat.date_today"]);
    assert.equal(dayLabel(Date.now() / 1000 - 86400), arabic["chat.date_yesterday"]);
  } finally { restore(); }
});

test("day separators use capitalized catalog labels for today and yesterday", () => {
  assert.equal(dayLabel(Date.now() / 1000), "Today");
  assert.equal(dayLabel(Date.now() / 1000 - 86400), "Yesterday");
});

test("list timestamps show time today, then yesterday, weekday and short date", () => {
  const now = new Date(2026, 10, 5, 15, 0);
  const at = (days: number) => new Date(2026, 10, 5 - days, 9, 30).getTime() / 1000;
  assert.equal(listStamp(at(0), now), formatTime(at(0)));
  assert.equal(listStamp(at(1), now), "Yesterday");
  assert.equal(listStamp(at(3), now), new Intl.DateTimeFormat("en", { weekday: "short" }).format(new Date(at(3) * 1000)));
  assert.equal(listStamp(at(30), now), new Intl.DateTimeFormat("en", { dateStyle: "short" }).format(new Date(at(30) * 1000)));
});
