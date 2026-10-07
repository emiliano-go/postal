import type { StorageFile, StorageReport, StorageCleanup } from "../../src/lib/utils/storage";
import type { Marks, StoredMessage } from "../../src/lib/utils/models";
import type { MessageCursor } from "../../src/lib/utils/message-window";
import type { CallRecord, ChannelPage, ChannelSummary, ChannelView } from "../../src/lib/utils/wire";
export const windowFixture = {
  archive: Array.from({ length: 350 }, (_, n) => ({ chat: "window@s", id: String(n).padStart(4, "0"), timestamp: 100, text: `Message ${n}` }) as StoredMessage),
  phoneRequests: 0, failure: false, deferNext: false, pending: [] as (() => void)[],
};
export const mediaFixture = { calls: 0, failure: true, deferNext: false, pending: [] as (() => void)[] };
export const callHistoryFixture = {
  calls: [] as { account: string; limit: number }[],
  records: [] as CallRecord[],
  failure: "",
  deferNext: false,
  pending: [] as (() => void)[],
};
export const pluginFixture = { enabled: false, failure: false, crashed: false };
export const storeFixture = { corrupt: true, failure: false, healthCalls: 0, recoveryCalls: 0,
  deferNext: false, pending: [] as (() => void)[] };
export const sendFixture = { before: null as ((command: string, args?: Record<string, unknown>) => Promise<void>) | null };
export const historyFixture = {
  enabled: true, delayNext: false, pending: [] as (() => void)[],
  offers: [] as { chat: unknown; account: unknown }[],
};
export const previewFixture = {
  calls: [] as string[], reads: [] as Record<string, unknown>[],
  deferNext: false, pending: [] as (() => void)[], failure: false,
  archive: Array.from({ length: 30 }, (_, n) => ({
    chat: "quiet@s.whatsapp.net", id: `preview-${n}`, timestamp: 1700000000 + n * 60,
    sender: "200@s.whatsapp.net", sender_name: "Saved sender", from_me: n % 3 === 2,
    text: n === 29 ? "Unread synthetic message with a long paragraph that stays readable across several lines. *Formatted text* and https://example.invalid stay passive. Second sentence must remain visible, with no truncation or chat selection."
      : n === 27 || n === 28 ? "HIDDEN SYNTHETIC CONTENT" : n === 26 ? "[image]" : `Unread synthetic message ${n}`,
    media_kind: n === 26 ? "image" : n === 27 ? "view_once" : n === 23 ? "audio" : n === 22 ? "video" : null,
    media_path: "C:\\synthetic\\unopened-media", media_thumb: "data:image/png;base64,AA==",
    system_kind: n === 20 ? "SYSTEM_NOTICE" : null,
    deleted: n === 24, revoked: n === 25, spoiler: n === 28, read: false, mentioned: n === 29,
  }) as StoredMessage),
};
export const dateJumpFixture = {
  pages: [] as StoredMessage[][],
  lookups: [] as { chat: unknown; start: unknown; end: unknown }[],
  requests: [] as Record<string, unknown>[],
  failure: false,
  deferNext: false,
  pending: [] as (() => void)[],
  deferMarks: false,
  pendingMarks: [] as (() => void)[],
};
export const pinFixture = {
  rows: [] as StoredMessage[],
  marks: null as Marks | null,
  calls: [] as { id: string; limit: number }[],
  failNextAnchor: "",
};
export const channelsFixture = {
  view: { channels: [] as ChannelSummary[], synced_at: null } as ChannelView,
  metadata: {} as Record<string, ChannelSummary>,
  pages: {} as Record<string, ChannelPage>,
  canPost: {} as Record<string, boolean>,
  calls: [] as { command: string; args?: Record<string, unknown> }[],
  failure: "",
  defer: "",
  pending: [] as (() => void)[],
};
export const selectionFixture = { calls: [] as { command: string; args: unknown }[], failure: false };
export const uploadFixture = { calls: [] as string[], maxChunk: 0, size: 0, written: 0, chunks: [] as Uint8Array[],
  failChunk: false, failSend: false, afterChunk: null as (() => void) | null };
export const archiveFixture = { failure: false, cancelled: false, deferNext: false, pending: [] as (() => void)[],
  calls: [] as string[], accounts: [{ id: "existing", label: "Existing account" }] as { id: string; label: string }[] };
export const fixture = { updated: false, failure: false, calls: 0, savedRetention: null as unknown,
  chatSettingsFailure: false, chatAutoDownload: null as boolean | null,
  chatSettingsDelay: false, chatSettingsPending: [] as (() => void)[],
  storageFailure: false, storageCalls: 0, cacheBytes: 512,
  media: [
    { chat: "a@s", id: "photo", kind: "image", filename: "photo.jpg", timestamp: 200, quoted: false, bytes: 2048, available: true },
    { chat: "a@s", id: "doc", kind: "document", filename: "notes.pdf", timestamp: 100, quoted: false, bytes: 1024, available: true },
    { chat: "b@s", id: "video", kind: "video", filename: "clip.mp4", timestamp: 300, quoted: false, bytes: 4096, available: true },
  ] as StorageFile[],
};

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  previewFixture.calls.push(command);
  if (command === "call_history") {
    callHistoryFixture.calls.push({ account: String(args?.account ?? ""), limit: Number(args?.limit ?? 0) });
    const failure = callHistoryFixture.failure;
    const records = structuredClone(callHistoryFixture.records);
    const complete = () => {
      if (failure) throw new Error(failure);
      return records as T;
    };
    if (callHistoryFixture.deferNext) {
      callHistoryFixture.deferNext = false;
      return new Promise<T>((resolve, reject) => callHistoryFixture.pending.push(() => {
        try { resolve(complete()); } catch (error) { reject(error); }
      }));
    }
    return complete();
  }
  if (["channels", "refresh_channels", "channel_metadata", "follow_channel", "unfollow_channel", "set_channel_muted", "set_channel_favorite", "channel_messages",
    "channel_can_post", "channel_post_text", "channel_post_media", "channel_post_poll", "channel_edit_text", "channel_revoke_post"].includes(command)) {
    channelsFixture.calls.push({ command, args });
    const complete = () => {
      if (channelsFixture.failure === command) throw new Error(`Synthetic ${command} failure`);
      const jid = String(args?.jid ?? "");
      const stored = channelsFixture.view.channels.find((channel) => channel.jid === jid);
      if (command === "channels" || command === "refresh_channels") return channelsFixture.view as T;
      if (command === "channel_can_post") return (channelsFixture.canPost[jid] ?? false) as T;
      if (["channel_post_text", "channel_post_media", "channel_post_poll", "channel_edit_text", "channel_revoke_post"].includes(command)) return undefined as T;
      if (command === "channel_metadata") {
        const channel = channelsFixture.metadata[jid];
        if (!channel) throw new Error("Synthetic channel not found");
        return channel as T;
      }
      if (command === "follow_channel") {
        const channel = channelsFixture.metadata[jid] ?? stored;
        if (!channel) throw new Error("Synthetic channel not found");
        const followed = { ...channel, followed: true };
        channelsFixture.view = { ...channelsFixture.view, channels: [...channelsFixture.view.channels.filter((row) => row.jid !== jid), followed] };
        channelsFixture.metadata[jid] = followed;
        return followed as T;
      }
      if (command === "unfollow_channel") {
        channelsFixture.view = { ...channelsFixture.view, channels: channelsFixture.view.channels.map((channel) => channel.jid === jid ? { ...channel, followed: false } : channel) };
        return undefined as T;
      }
      if (command === "set_channel_muted") {
        const channel = channelsFixture.metadata[jid] ?? stored;
        if (!channel) throw new Error("Synthetic channel not found");
        const muted = { ...channel, muted: Boolean(args?.muted) };
        channelsFixture.view = { ...channelsFixture.view, channels: channelsFixture.view.channels.map((row) => row.jid === jid ? muted : row) };
        channelsFixture.metadata[jid] = muted;
        return muted as T;
      }
      if (command === "set_channel_favorite") {
        const channel = channelsFixture.metadata[jid] ?? stored;
        if (!channel) throw new Error("Synthetic channel not found");
        const favorite = { ...channel, favorite: Boolean(args?.favorite) };
        channelsFixture.view = { ...channelsFixture.view, channels: channelsFixture.view.channels.map((row) => row.jid === jid ? favorite : row) };
        channelsFixture.metadata[jid] = favorite;
        return favorite as T;
      }
      const page = channelsFixture.pages[`${jid}:${String(args?.before ?? "")}`] ?? channelsFixture.pages[jid]
        ?? { messages: [], next_before: null, has_more: false };
      for (const message of page.messages) {
        if (!windowFixture.archive.some((row) => row.chat === jid && row.id === message.id)) windowFixture.archive.push(message);
      }
      return page as T;
    };
    if (channelsFixture.defer === command) {
      channelsFixture.defer = "";
      return new Promise<T>((resolve, reject) => channelsFixture.pending.push(() => {
        try { resolve(complete()); } catch (error) { reject(error); }
      }));
    }
    return complete();
  }
  if (command === "message_store_health") {
    storeFixture.healthCalls++;
    const health = { status: storeFixture.corrupt ? "corrupt" : "healthy", path: "synthetic/messages.db",
      diagnosis: storeFixture.corrupt ? "Synthetic corrupted page" : null };
    if (storeFixture.deferNext) {
      storeFixture.deferNext = false;
      return new Promise<T>((resolve) => storeFixture.pending.push(() => resolve(health as T)));
    }
    return health as T;
  }
  if (command === "recover_message_store") {
    storeFixture.recoveryCalls++;
    if (args?.account !== "settings-fixture") throw { kind: "postal_error", code: "error.unknown_account", params: {} };
    if (storeFixture.failure) throw { kind: "postal_error", code: "error.message_store_recovery_failed", params: {}, diagnostic: "Synthetic rename failure; original files retained" };
    storeFixture.corrupt = false;
    return { preserved_directory: "synthetic/recovery-preserved", restart_diagnostic: null } as T;
  }
  if (command === "group_history_offer") {
    historyFixture.offers.push({ chat: args?.chat, account: args?.account });
    const offer = { enabled: historyFixture.enabled, reason: historyFixture.enabled ? null : "WhatsApp disabled group history.", max_messages: 100, time_window_seconds: 7 * 86400 };
    if (historyFixture.delayNext) {
      historyFixture.delayNext = false;
      return new Promise<T>((resolve) => historyFixture.pending.push(() => resolve(offer as T)));
    }
    return offer as T;
  }
  if (command === "search") return ["Alice", "Bob", "Carol"].map((name, index) => ({
    jid: `${100 + index}@s.whatsapp.net`, name, number: `${100 + index}`, kind: "contact", saved: true, has_messages: true, aliases: [],
  })) as T;
  if (["star", "react", "forward_message", "delete_messages"].includes(command)) {
    const target = args?.target as { chat?: string } | undefined;
    if ((target?.chat ?? args?.chat) === "selection@s.whatsapp.net") {
      selectionFixture.calls.push({ command, args });
      if (selectionFixture.failure) throw new Error("Synthetic selection failure");
      return undefined as T;
    }
  }
  if (sendFixture.before) await sendFixture.before(command, args);
  if (["send_text", "send_reply", "send_voice", "send_sticker", "send_from_library", "send_typing"].includes(command)) return undefined as T;
  if (command === "list_plugins") return { directory: "synthetic/plugins", errors: [], plugins: [
    { id: "com.example.fixture", name: "Synthetic Plugin", version: "1", activation: "lazy", idle_timeout_secs: 30,
      capabilities: ["events:read"], enabled: pluginFixture.enabled, state: "idle", error: pluginFixture.crashed ? "Disabled after three synthetic crashes" : null,
      error_code: null, limits: { windows_job_commit_gib: 4, unix_process_address_space_gib: 4,
        process_cpu_minutes: 30, windows_max_processes: 8, unix_max_processes: null } },
  ] } as T;
  if (command === "set_plugin_enabled") {
    if (pluginFixture.failure) throw new Error("Synthetic consent persistence failure");
    if (args?.enabled && JSON.stringify(args.capabilities) !== '["events:read"]') throw new Error("Missing consent");
    pluginFixture.enabled = Boolean(args?.enabled); pluginFixture.crashed = false;
    return undefined as T;
  }
  if (command === "begin_upload") {
    uploadFixture.calls.push("begin"); uploadFixture.size = Number(args?.size); uploadFixture.written = 0; uploadFixture.chunks = [];
    return "synthetic-upload" as T;
  }
  if (command === "append_upload") {
    uploadFixture.calls.push("append");
    if (uploadFixture.failChunk) throw new Error("Synthetic staging failure");
    if (args?.offset !== uploadFixture.written) throw new Error("Out-of-order chunk");
    const bytes = Uint8Array.from(atob(String(args?.data)), (character) => character.charCodeAt(0));
    uploadFixture.maxChunk = Math.max(uploadFixture.maxChunk, bytes.length);
    uploadFixture.written += bytes.length; uploadFixture.chunks.push(bytes);
    uploadFixture.afterChunk?.();
    return undefined as T;
  }
  if (command === "send_media") {
    uploadFixture.calls.push(args?.upload ? "send-staged" : "send-inline");
    if (uploadFixture.failSend) throw new Error("Synthetic send failure");
    if (args?.upload && uploadFixture.written !== uploadFixture.size) throw new Error("Incomplete staged upload");
    return null as T;
  }
  if (command === "cancel_upload") { uploadFixture.calls.push("cancel"); return undefined as T; }
  if (command === "chats") return [{ chat: "archive@s", display_name: "Synthetic conversation" }] as T;
  if (command === "accounts") return { accounts: archiveFixture.accounts, active: "existing" } as T;
  if (command === "export_archive" || command === "restore_local_backup") {
    archiveFixture.calls.push(`${command}:${args?.chat ?? "all"}`);
    const complete = () => {
      if (archiveFixture.cancelled) return null as T;
      if (archiveFixture.failure) throw new Error("Synthetic archive write failure");
      if (command === "restore_local_backup") archiveFixture.accounts.push({ id: "restored", label: "Restored backup" });
      return { directory: "synthetic-output", messages: args?.chat ? 2 : 1003, attachments: 1, missing_attachments: 1 } as T;
    };
    if (archiveFixture.deferNext) {
      archiveFixture.deferNext = false;
      return new Promise<T>((resolve, reject) => archiveFixture.pending.push(() => {
        try { resolve(complete()); } catch (error) { reject(error); }
      }));
    }
    return complete();
  }
  if (command === "download_media") {
    mediaFixture.calls++;
    const complete = () => {
      if (mediaFixture.failure) throw new Error("Synthetic sender unavailable");
      const message = windowFixture.archive.find((m) => m.chat === args?.chat && m.id === args?.id);
      if (message) message.media_path = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
      return undefined as T;
    };
    if (mediaFixture.deferNext) {
      mediaFixture.deferNext = false;
      return new Promise<T>((resolve, reject) => mediaFixture.pending.push(() => {
        try { resolve(complete()); } catch (error) { reject(error); }
      }));
    }
    return complete();
  }
  if (command === "message_page") {
    if (args?.chat === "pins-fixture@s.whatsapp.net") {
      const id = String(args.anchorId ?? "");
      pinFixture.calls.push({ id, limit: Number(args.limit ?? 0) });
      if (id && id === pinFixture.failNextAnchor) { pinFixture.failNextAnchor = ""; return { messages: [], has_more: false } as T; }
      const row = pinFixture.rows.find((message) => message.id === id);
      return { messages: row ? [row] : [], has_more: false } as T;
    }
    if (args?.chat === "quiet@s.whatsapp.net") {
      previewFixture.reads.push({ ...args });
      if (previewFixture.failure) throw new Error("Synthetic preview failure");
      const limit = Number(args?.limit ?? 30);
      const result = { messages: previewFixture.archive.toReversed().slice(0, limit), has_more: previewFixture.archive.length > limit } as T;
      if (previewFixture.deferNext) {
        previewFixture.deferNext = false;
        return new Promise<T>((resolve) => previewFixture.pending.push(() => resolve(result)));
      }
      return result;
    }
    if (windowFixture.failure) throw new Error("Synthetic page failure");
    const compare = (a: MessageCursor, b: MessageCursor) => a.timestamp - b.timestamp || (a.sort_order ?? 0) - (b.sort_order ?? 0) || (a.id === b.id ? 0 : a.id < b.id ? -1 : 1);
    const anchor = args?.anchorId ? windowFixture.archive.find((m) => m.chat === args?.chat && m.id === args.anchorId) : undefined;
    const cursor = anchor ?? args?.cursor as MessageCursor | undefined;
    const after = args?.direction === "after";
    const rows = windowFixture.archive.filter((m) => m.chat === args?.chat && (!args?.anchorId || anchor) &&
      (!cursor || (after ? compare(m, cursor) > 0 : args?.direction === "through" ? compare(m, cursor) <= 0 : compare(m, cursor) < 0)))
      .sort((a, b) => after ? compare(a, b) : compare(b, a));
    const limit = Number(args?.limit ?? 100);
    const messages = rows.slice(0, limit);
    if (after) messages.reverse();
    const result = { messages, has_more: rows.length > limit } as T;
    if (windowFixture.deferNext) {
      windowFixture.deferNext = false;
      return new Promise<T>((resolve) => windowFixture.pending.push(() => resolve(result)));
    }
    return result;
  }
  if (command === "load_older") { windowFixture.phoneRequests++; return undefined as T; }
  if (command === "marks") {
    if (args?.chat === "pins-fixture@s.whatsapp.net" && pinFixture.marks) return pinFixture.marks as T;
    if (args?.chat === "date@s" && dateJumpFixture.deferMarks) {
      dateJumpFixture.deferMarks = false;
      const marks = { reactions: [], starred: [], pinned: null, pinned_messages: [], polls: [], events: [], view_once: [], forwarded: [], edited: [] };
      return new Promise<T>((resolve) => dateJumpFixture.pendingMarks.push(() => resolve(marks as T)));
    }
    return { reactions: [], starred: [], pinned: null, pinned_messages: [], polls: [], events: [], view_once: [], forwarded: [], edited: [] } as T;
  }
  if (command === "message_on_date") {
    dateJumpFixture.lookups.push({ chat: args?.chat, start: args?.start, end: args?.end });
    if (dateJumpFixture.failure) throw new Error("Synthetic local date lookup failure");
    return (windowFixture.archive.filter((message) => message.chat === args?.chat
      && message.timestamp >= Number(args?.start) && message.timestamp < Number(args?.end))
      .toSorted((a, b) => a.timestamp - b.timestamp || (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.id.localeCompare(b.id))[0] ?? null) as T;
  }
  if (command === "load_older_for_date") {
    dateJumpFixture.requests.push({ ...args });
    const complete = () => {
      if (dateJumpFixture.failure) throw new Error("Synthetic date history failure");
      const page = dateJumpFixture.pages.shift() ?? [];
      let added = false;
      for (const message of page) if (!windowFixture.archive.some((row) => row.chat === message.chat && row.id === message.id)) {
        windowFixture.archive.push(message); added = true;
      }
      return added as T;
    };
    if (dateJumpFixture.deferNext) {
      dateJumpFixture.deferNext = false;
      return new Promise<T>((resolve, reject) => dateJumpFixture.pending.push(() => {
        try { resolve(complete()); } catch (error) { reject(error); }
      }));
    }
    return complete();
  }
  if (command === "storage_report") {
    let files = fixture.media.filter((file) => !args?.chat || file.chat === args.chat)
      .toSorted((a, b) => args?.order === "oldest" ? a.timestamp - b.timestamp : b.bytes - a.bytes);
    const total_files = files.length;
    const offset = Number(args?.offset ?? 0);
    files = files.slice(offset, offset + 50).map((file) => ({ ...file }));
    const report: StorageReport = {
      database_bytes: 65536, attachment_bytes: fixture.media.reduce((sum, file) => sum + file.bytes, 0),
      cache_bytes: fixture.cacheBytes, other_bytes: 128, total_files, files,
      chats: ["a@s", "b@s"].map((chat) => {
        const by_kind: Record<string, number> = {};
        for (const file of fixture.media.filter((file) => file.chat === chat)) by_kind[file.kind] = (by_kind[file.kind] ?? 0) + file.bytes;
        return { chat, name: chat === "a@s" ? "Synthetic A" : "Synthetic B", by_kind,
          bytes: Object.values(by_kind).reduce((sum, bytes) => sum + bytes, 0) };
      }),
    };
    return report as T;
  }
  if (command === "storage_cleanup") {
    fixture.storageCalls++;
    if (fixture.storageFailure) throw new Error("Synthetic filesystem failure");
    const action = args?.action as StorageCleanup;
    if (action.kind === "cache") {
      const bytes = fixture.cacheBytes;
      fixture.cacheBytes = 0;
      return { files: bytes ? 1 : 0, bytes } as T;
    }
    const removed = fixture.media.filter((file) => file.chat === action.chat &&
      (action.kind === "chat_media" || (file.id === action.id && file.quoted === action.quoted)));
    fixture.media = fixture.media.filter((file) => !removed.includes(file));
    return { files: removed.length, bytes: removed.reduce((sum, file) => sum + file.bytes, 0) } as T;
  }
  if (command === "chat_settings") {
    if (fixture.chatSettingsDelay) {
      fixture.chatSettingsDelay = false;
      await new Promise<void>((resolve) => fixture.chatSettingsPending.push(resolve));
    }
    if (fixture.chatSettingsFailure) throw new Error("Synthetic chat settings failure");
    return {
    auto_download: fixture.chatAutoDownload,
    auto_download_types: { image: fixture.chatAutoDownload, video: fixture.chatAutoDownload, audio: fixture.chatAutoDownload,
      document: fixture.chatAutoDownload, sticker: fixture.chatAutoDownload, gif: fixture.chatAutoDownload },
    sound_muted: null,
    unarchive: null,
    send_typing: null,
    send_receipts: null,
    mute_at_all: false,
    retention: { max_age_hours: { kind: "inherit" }, max_messages: { kind: "limited", value: 200 }, on_demand: true },
    } as T;
  }
  if (command === "set_chat_retention") {
    fixture.savedRetention = JSON.parse(JSON.stringify(args?.retention));
    return undefined as T;
  }
  if (command === "once_state") return {
    paired: false, pairing: false, running: false, connected: false, qr: null,
  } as T;
  if (command === "set_pairing") return undefined as T;
  if (command !== "boolean_props") throw new Error(`No synthetic response for ${command}`);
  fixture.calls++;
  if (fixture.failure) throw new Error("Synthetic disconnected account");
  return [
    { name: "example_enabled", code: 1, default: false, value: !fixture.updated },
    { name: "example_disabled", code: 2, default: true, value: false },
    { name: "example_missing", code: 3, default: true, value: null },
  ] as T;
}

export function log(_level: string, _message: string) {}
