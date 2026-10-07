import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { t } from "../lib/i18n/localizer.ts";

test("notification delivery stays account-bound through permission and sound awaits", async () => {
  const source = readFileSync(new URL("../lib/utils/notifications.ts", import.meta.url), "utf8");
  const js = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
  }).outputText;
  const compiled = js.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
  const calls: { command: string; args: unknown }[] = [];
  const shown: { title: string; options: unknown }[] = [];
  const played: string[] = [];
  let permission: () => Promise<boolean> = async () => true;
  let invokeImpl: (command: string, args: unknown) => unknown = async () => undefined;
  class FakeNotification {
    static permission = "granted";
    onclick: (() => void) | null = null;
    readonly title: string;
    readonly options: unknown;
    constructor(title: string, options: unknown) {
      this.title = title;
      this.options = options;
      shown.push({ title, options });
    }
  }
  const windowStub = { focus() {}, dispatchEvent() {} };
  const exports = new Function(
    "isPermissionGranted", "requestPermission", "sendNotification", "invoke", "plain",
    "MEDIA_LABELS", "captionOf", "Notification", "window", "CustomEvent", "t", "playNotificationSound",
    `${compiled}\nreturn { showChatNotification };`,
  )(
    () => permission(), async () => true, async () => {},
    (command: string, args: unknown) => { calls.push({ command, args }); return invokeImpl(command, args); },
    (text: string) => text, {}, () => "", FakeNotification, windowStub, class {}, t,
    async (sound: string, current: () => boolean) => { if (current()) played.push(sound); return current(); },
  ) as { showChatNotification: (title: string, body: string, chat: string, accountId: string, current: () => boolean, fallbackSound?: () => string) => Promise<void> };

  let current = true;
  let fallbackSound = "chime";
  const notify = (title = "Title", body = "Body") => exports.showChatNotification(title, body, "chat", "account-a", () => current, () => fallbackSound);
  let resolvePermission!: (granted: boolean) => void;
  permission = () => new Promise((resolve) => { resolvePermission = resolve; });
  const staleNative = notify();
  current = false;
  resolvePermission(true);
  await staleNative;
  assert.deepEqual(calls, []);

  current = true;
  permission = async () => true;
  invokeImpl = async () => undefined;
  await notify();
  assert.deepEqual(calls, [{
    command: "show_chat_notification",
    args: { accountId: "account-a", chat: "chat", title: "Title", body: "Body" },
  }]);
  assert.deepEqual(played, []);

  invokeImpl = async (command) => command === "show_chat_notification" ? "chime" : undefined;
  await notify();
  assert.deepEqual(played, ["chime"]);

  invokeImpl = async (command) => command === "show_chat_notification" ? null : undefined;
  await notify();
  assert.deepEqual(played, ["chime"]);

  invokeImpl = async (command) => {
    if (command === "show_chat_notification") throw new Error("native unavailable");
    return true;
  };
  await notify();
  assert.deepEqual(shown[0], { title: "Title", options: { body: "Body", tag: "postal-chat", silent: true } });
  assert.deepEqual(calls.slice(-1)[0], { command: "chat_sound_muted", args: { accountId: "account-a", chat: "chat" } });
  assert.deepEqual(played, ["chime"]);

  invokeImpl = async (command) => {
    if (command === "show_chat_notification") throw new Error("native unavailable");
    return null;
  };
  await notify();
  assert.deepEqual(shown[1], { title: "Title", options: { body: "Body", tag: "postal-chat", silent: true } });
  assert.deepEqual(played, ["chime", "chime"]);

  fallbackSound = "system";
  await notify();
  assert.deepEqual(shown[2], { title: "Title", options: { body: "Body", tag: "postal-chat", silent: false } });
  assert.deepEqual(played, ["chime", "chime"]);
  fallbackSound = "chime";

  invokeImpl = async (command) => {
    if (command === "show_chat_notification") throw new Error("native unavailable");
    throw new Error("mute lookup failed");
  };
  const shownBeforeMuteFailure = shown.length;
  const playedBeforeMuteFailure = played.length;
  await notify();
  assert.equal(shown.length, shownBeforeMuteFailure);
  assert.equal(played.length, playedBeforeMuteFailure);

  let resolveSound!: (muted: boolean) => void;
  invokeImpl = async (command) => {
    if (command === "show_chat_notification") throw new Error("native unavailable");
    return new Promise((resolve) => { resolveSound = resolve; });
  };
  const shownBeforeStale = shown.length;
  const staleWeb = notify();
  await new Promise<void>((resolve) => setImmediate(resolve));
  current = false;
  resolveSound(true);
  await staleWeb;
  assert.equal(shown.length, shownBeforeStale);
  assert.equal(played.length, playedBeforeMuteFailure);

  let resolveDisplay!: (sound: string | null) => void;
  current = true;
  invokeImpl = async (command) => command === "show_chat_notification"
    ? new Promise((resolve) => { resolveDisplay = resolve; })
    : null;
  const playedBeforeDisplay = played.length;
  const staleDisplay = notify();
  await new Promise<void>((resolve) => setImmediate(resolve));
  current = false;
  resolveDisplay("chime");
  await staleDisplay;
  assert.equal(played.length, playedBeforeDisplay);

  current = true;
  permission = async () => false;
  const callsBeforeDenied = calls.length;
  await notify();
  assert.equal(calls.length, callsBeforeDenied);
});
