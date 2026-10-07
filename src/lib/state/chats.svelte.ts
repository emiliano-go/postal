// Chats domain: the conversation list, selection, search, pins, avatars and
// the group-info panel. Moved out of +page.svelte. Reads members (names) and
// session (self, accounts); cross-domain flows (openChat) stay in the route.
import { invoke } from "$lib/utils/ipc";
import type { ChatPage } from "$lib/utils/wire";
import { mergeSummaries } from "$lib/utils/chat-list";
import { plain } from "$lib/utils/format";
import { bare, MEDIA_LABELS } from "$lib/utils/message";
import type { IconName } from "$lib/ui/Icon.svelte";
import type {
  Account,
  ChatFilter,
  ChatSummary,
  GroupInfo,
  SearchResult,
} from "$lib/utils/models";
import { members } from "./members.svelte";
import { session } from "./session.svelte";
import { ui } from "./ui.svelte";
import { LocalizedError, normalizeError } from "../i18n/errors.ts";
import { uiError } from "./localized.ts";
import { t } from "../i18n/localizer.ts";

function bareJid(jid: string) {
  return jid.replace(/@.*$/, "");
}

export class ChatsState {
  /** Summaries seen in loaded pages or on-demand whole-account views. */
  chats: ChatSummary[] = $state.raw([]);
  sidebarRows: ChatSummary[] = $state.raw([]);
  sidebarCursor = $state<string | null>(null);
  sidebarLoading = $state(false);
  sidebarTotals = $state.raw({ archived: 0, unreadChats: 0, unreadMentions: 0, desktopUnread: 0 });
  private sidebarAllowed: string[] | null = null;
  private sidebarOrderAllowed = false;
  private sidebarIncludeArchived = false;
  private sidebarScope = "";
  private pageSeq = 0;
  private summarySeq = 0;
  /** Chat list only: cheap, local, never blocks on the network. */
  chatsSeq = 0;
  selectedChat = $state<string | null>(null);
  titleOverride = $state<string | null>(null);
  chatFilter = $state<ChatFilter>("all");
  labelFilter = $state("");

  searchQuery = $state("");
  searchResults = $state.raw<SearchResult[]>([]);
  searchTimer: ReturnType<typeof setTimeout> | undefined = undefined;
  searchSeq = 0;

  /** Communities and their subgroups among our groups, for the chat list. */
  groupKinds = $state.raw<
    Record<string, { community: boolean; announcements: boolean; parent: string | null }>
  >({});

  showGroupInfo = $state(false);
  auditRevision = $state(0);
  groupInfo = $state<GroupInfo | null>(null);
  #groupInfoError = $state<LocalizedError | null>(null);
  get groupInfoError(): string | null { return this.#groupInfoError?.message ?? null; }
  set groupInfoError(value: unknown) { this.#groupInfoError = value == null ? null : normalizeError(value); }
  get groupInfoDiagnostic() { return this.#groupInfoError?.diagnostic; }

  /** Cached profile picture per chat; `null` means it has none. */
  avatars: Record<string, string | null> = $state({});
  requestedAvatars = new Set<string>();
  avatarQueue: string[] = [];
  avatarWorkers = 0;

  /** Coalesces an event burst into at most one chat-list reload per 200 ms. */

  archivedChats = $derived(this.sidebarTotals.archived);
  unreadChats = $derived(this.sidebarTotals.unreadChats);
  unreadPings = $derived(this.sidebarTotals.unreadMentions);

  /** Each account's own picture as last seen, so it shows before that account connects. */
  accountAvatars = $derived(
    Object.fromEntries(
      session.accountList.map((a) => {
        const jid = a.id === session.activeAccount ? (session.me ?? a.jid) : a.jid;
        return [a.id, (jid ? this.avatars[jid] : null) ?? this.rememberedAvatar(a.id)];
      }),
    ) as Record<string, string | null>,
  );

  chatLabel(chat: ChatSummary) {
    return members.displayName(chat.display_name, chat.chat);
  }

  /** Who sent a chat's last message, as the list prefixes it. */
  previewAuthor(chat: ChatSummary) {
    if (chat.last_from_me) return t("chat.you");
    if (!chat.chat.endsWith("@g.us") || chat.last_media_kind === "missed_call") return null;
    return members.displayName(chat.last_sender_name, chat.last_sender);
  }

  /** The last message's text; media without a caption reads as its kind. */
  previewText(chat: ChatSummary) {
    const kind = chat.last_media_kind;
    if (kind === "poll") return `📊 ${chat.last_text}`;
    if (kind === "event") return `📅 ${chat.last_text}`;
    if (kind === "view_once") return t("state.view_once");
    if (kind === "missed_call") return t("state.missed_call");
    if (!kind || chat.last_text.trim() !== `[${kind}]`) {
      return plain(chat.last_text, (user) => members.mentionName(user));
    }
    return MEDIA_LABELS[kind] ?? t("state.attachment");
  }

  mediaIcon(kind: string | null): IconName | null {
    if (kind === "image" || kind === "sticker") return "image";
    if (kind === "video" || kind === "gif") return "video";
    if (kind === "audio") return "mic";
    if (kind === "document") return "file";
    if (kind === "location" || kind === "live_location") return "pin";
    return null;
  }

  /** The display name of a chat, for cross chat quotes. */
  chatName(jid: string) {
    return members.displayName(this.chats.find((c) => c.chat === jid)?.display_name ?? null, jid);
  }

  async refreshChats() {
    const seq = ++this.chatsSeq;
    const scope = this.sidebarScope;
    const depth = Math.max(this.sidebarRows.length, 64);
    const oldRows = this.sidebarRows;
    const pageSeq = ++this.pageSeq;
    this.sidebarLoading = true;
    try {
      const rows: ChatSummary[] = [];
      let after: string | null = null;
      let first: ChatPage | null = null;
      do {
        const page: ChatPage = await invoke<ChatPage>("chats_page", { muteAllAtAll: session.settings.mute_all_at_all ?? false,
          filter: this.sidebarIncludeArchived && this.chatFilter === "all" ? "space_all" : this.chatFilter,
          allowedChats: this.sidebarAllowed, orderAllowed: this.sidebarOrderAllowed, after, limit: Math.min(100, depth - rows.length) });
        if (seq !== this.chatsSeq || scope !== this.sidebarScope) return;
        first ??= page;
        rows.push(...page.rows);
        after = page.next_cursor;
      } while (after && rows.length < depth);
      const oldIds = new Set(oldRows.map((row) => row.chat));
      this.sidebarRows = mergeSummaries(oldRows, rows);
      this.sidebarCursor = after;
      this.sidebarTotals = { archived: first!.archived_count, unreadChats: first!.unread_chats,
        unreadMentions: first!.unread_mentions, desktopUnread: first!.desktop_unread };
      this.chats = mergeSummaries(this.chats, [...rows, ...this.chats.filter((row) => !oldIds.has(row.chat))]);
    } catch (e) {
      ui.fail(e);
    } finally { if (pageSeq === this.pageSeq) this.sidebarLoading = false; }
  }

  setSidebarScope(allowed: string[] | null, orderAllowed = false, includeArchived = false) {
    const scope = JSON.stringify([this.chatFilter, allowed, orderAllowed, includeArchived]);
    if (scope === this.sidebarScope) return;
    this.sidebarScope = scope;
    this.sidebarAllowed = allowed;
    this.sidebarOrderAllowed = orderAllowed;
    this.sidebarIncludeArchived = includeArchived;
    this.sidebarRows = [];
    this.sidebarCursor = null;
    this.sidebarLoading = false;
    this.pageSeq++;
    void this.refreshChats();
  }

  async loadMoreChats() {
    const after = this.sidebarCursor;
    if (!after || this.sidebarLoading) return;
    const scope = this.sidebarScope, seq = ++this.pageSeq;
    this.sidebarLoading = true;
    try {
      const page = await invoke<ChatPage>("chats_page", { muteAllAtAll: session.settings.mute_all_at_all ?? false,
        filter: this.sidebarIncludeArchived && this.chatFilter === "all" ? "space_all" : this.chatFilter,
        allowedChats: this.sidebarAllowed, orderAllowed: this.sidebarOrderAllowed, after, limit: 64 });
      if (seq !== this.pageSeq || scope !== this.sidebarScope) return;
      const known = new Set(this.sidebarRows.map((row) => row.chat));
      this.sidebarRows = mergeSummaries(this.sidebarRows, [...this.sidebarRows, ...page.rows.filter((row) => !known.has(row.chat))]);
      this.sidebarCursor = page.next_cursor;
      const cache = new Map(this.chats.map((row) => [row.chat, row]));
      for (const row of page.rows) cache.set(row.chat, row);
      this.chats = mergeSummaries(this.chats, [...cache.values()]);
    } catch (error) { if (scope === this.sidebarScope) ui.fail(error); }
    finally { if (seq === this.pageSeq && scope === this.sidebarScope) this.sidebarLoading = false; }
  }

  async allChats(updateCache = true): Promise<ChatSummary[]> {
    const account = session.activeAccount;
    if (!account) throw uiError("error.not_connected");
    const rows = await invoke<ChatSummary[]>("chats", { accountId: account, muteAllAtAll: session.settings.mute_all_at_all ?? false });
    if (updateCache && account === session.activeAccount) this.chats = mergeSummaries(this.chats, rows);
    return rows;
  }

  async hydrateChat(chat: string) {
    const account = session.activeAccount, seq = ++this.summarySeq;
    try {
      const page = await invoke<ChatPage>("chats_page", { muteAllAtAll: session.settings.mute_all_at_all ?? false,
        filter: "space_all", allowedChats: [chat], orderAllowed: false, after: null, limit: 1 });
      if (account !== session.activeAccount || seq !== this.summarySeq || chat !== this.selectedChat) return;
      const row = page.rows[0];
      if (row) this.chats = mergeSummaries(this.chats, [row, ...this.chats.filter((item) => item.chat !== chat)]);
    } catch (error) { if (account === session.activeAccount && chat === this.selectedChat) ui.fail(error); }
  }

  /** Runs the chat/contact/group search, debounced while the user types. */
  runSearch() {
    clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => void this.searchNow(), 200);
  }

  private async searchNow() {
    const seq = ++this.searchSeq;
    const query = this.searchQuery.trim();
    if (!query) {
      this.searchResults = [];
      return;
    }
    try {
      const results = await invoke<SearchResult[]>("search", { query });
      if (seq === this.searchSeq) this.searchResults = results;
    } catch (e) {
      if (seq === this.searchSeq) ui.fail(e);
    }
  }

  clearSearch() {
    clearTimeout(this.searchTimer);
    this.searchSeq++;
    this.searchQuery = "";
    this.searchResults = [];
  }

  /** Pins or unpins a chat, mirrored to the account. */
  async togglePin(chat: ChatSummary, event?: MouseEvent) {
    event?.stopPropagation();
    try {
      await invoke("set_pinned", { chat: chat.chat, pinned: !chat.pinned });
      await this.refreshChats();
    } catch (e) {
      ui.fail(e);
    }
  }

  /** Runs a chat-state command (archive, mute, unread mark, leave) and reloads the list. */
  async chatAction(command: string, args: Record<string, unknown>) {
    try {
      await invoke(command, args);
      await this.refreshChats();
    } catch (e) {
      ui.fail(e);
    }
  }

  /** Clears one chat on this device only: messages go, the empty chat stays. */
  async clearChat(chat: string) {
    try {
      await invoke("clear_chat", { chat });
      await this.refreshChats();
      return true;
    } catch (e) {
      ui.fail(e);
      return false;
    }
  }

  /** Deletes one chat on this device only: it leaves the list until a new message arrives. */
  async deleteChat(chat: string) {
    try {
      await invoke("delete_chat", { chat });
      if (this.selectedChat === chat) {
        this.selectedChat = null;
        this.titleOverride = null;
        this.showGroupInfo = false;
        this.groupInfo = null;
      }
      await this.refreshChats();
      return true;
    } catch (e) {
      ui.fail(e);
      return false;
    }
  }

  async loadGroupKinds() {
    try {
      this.groupKinds = await invoke("group_kinds");
    } catch {
      // Not connected yet; the next connection loads them.
    }
  }

  /** Opens the right sidebar with the group's subject, description and members. */
  async openGroupInfo() {
    const selectedChat = this.selectedChat;
    if (!selectedChat) return;
    this.showGroupInfo = true;
    this.groupInfo = null;
    this.groupInfoError = null;
    try {
      this.groupInfo = await invoke<GroupInfo>("group_info", { chat: selectedChat });
    } catch (e) {
      // The query can time out on a busy server; keep the panel open so the
      // failure is visible and retryable rather than looking like a dead click.
      this.groupInfoError = e;
    }
  }

  /** Fetches a profile picture once, a few requests at a time. */
  loadAvatar(jid: string) {
    if (this.requestedAvatars.has(jid)) return;
    this.requestedAvatars.add(jid);
    this.avatarQueue.push(jid);
    while (this.avatarWorkers < 4 && this.avatarQueue.length > 0) void this.avatarWorker();
  }

  async avatarWorker() {
    this.avatarWorkers++;
    try {
      for (let jid = this.avatarQueue.shift(); jid; jid = this.avatarQueue.shift()) {
        try {
          this.avatars[jid] = await invoke<string | null>("avatar", { jid });
        } catch {
          this.avatars[jid] = null;
        }
      }
    } finally {
      this.avatarWorkers--;
    }
  }

  /** A JID's cached picture, fetching it on first use. */
  pictureOf(jid: string) {
    this.loadAvatar(jid);
    return this.avatars[jid] ?? null;
  }

  /** Drops one cached picture so the next read fetches it again. */
  forgetAvatar(jid: string) {
    this.requestedAvatars.delete(jid);
    delete this.avatars[jid];
  }

  rememberedAvatar(id: string): string | null {
    try {
      return localStorage.getItem(`postal.avatar.${id}`);
    } catch {
      return null;
    }
  }

  /** Mirrors resetUi: list, selection, pictures and the group panel are dropped. */
  resetAccount() {
    this.chatsSeq++;
    this.pageSeq++;
    this.summarySeq++;
    this.labelFilter = "";
    this.clearSearch();
    this.chats = [];
    this.sidebarRows = [];
    this.sidebarCursor = null;
    this.sidebarTotals = { archived: 0, unreadChats: 0, unreadMentions: 0, desktopUnread: 0 };
    this.sidebarScope = "";
    this.avatars = {};
    this.requestedAvatars.clear();
    this.selectedChat = null;
    this.groupInfo = null;
    this.showGroupInfo = false;
  }
}

export const chats = new ChatsState();

export type { Account };
