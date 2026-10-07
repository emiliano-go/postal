<script lang="ts">
  import { t, formatNumber } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { untrack } from "svelte";
  import { compareMessages } from "$lib/utils/message-window";
  import { invoke } from "$lib/utils/ipc";
  import Avatar from "$lib/ui/Avatar.svelte";
  import { appliableLabels, inboxCategories, inboxChats } from "$lib/utils/inbox";
  import type { InboxAction, InboxFilters, InboxLabel } from "$lib/utils/inbox";
  import type { ChatSummary, MessageLabelAssociation, StoredMessage } from "$lib/utils/wire";

  let { account, requestKey = 0, connected, chats, loading = false, error = "", labels = null,
    labelsByChat = {}, messageLabels = [], labelsWritable = false, labelsLoading = false, labelsComplete = false, labelsError = "", chatLabelOf, avatarOf = () => null,
    previewTextOf = (chat) => chat.last_text, formatTime, syncPending = 0, syncApplied = 0,
    historyPercent = null, backfill = null, finalizing = false, onopen, onopenmessage, onaction, onretry, initialFilters, onfilterschange }: {
    account: string | null;
    requestKey?: string | number;
    connected: boolean;
    chats: ChatSummary[];
    loading?: boolean;
    error?: LocalizedError | string | null;
    labels?: InboxLabel[] | null;
    labelsByChat?: Readonly<Record<string, readonly string[]>>;
    messageLabels?: readonly MessageLabelAssociation[];
    labelsWritable?: boolean;
    labelsLoading?: boolean;
    labelsComplete?: boolean;
    labelsError?: LocalizedError | string | null;
    chatLabelOf: (chat: ChatSummary) => string;
    avatarOf?: (jid: string) => string | null;
    previewTextOf?: (chat: ChatSummary) => string;
    formatTime: (timestamp: number) => string;
    syncPending?: number;
    syncApplied?: number;
    historyPercent?: number | null;
    backfill?: { done: number; total: number } | null;
    finalizing?: boolean;
    onopen: (chat: string, mention?: boolean) => void;
    onopenmessage?: (chat: string, id: string) => void;
    onaction: (account: string, chat: string, action: InboxAction) => Promise<void>;
    onretry?: () => void;
    initialFilters?: InboxFilters;
    onfilterschange?: (filters: InboxFilters) => void;
  } = $props();

  const kinds = [["unread", "chat.unread"], ["mentions", "chat.mentions"], ["labelled", "chat.labelled"], ["muted", "chat.muted"], ["archived", "chat.archived"]] as const;
  const emptyFilters = (): InboxFilters => ({ unread: false, mentions: false, labelled: false, muted: false, archived: false, label: "", query: "" });
  let filters = $state(emptyFilters());
  let busy = $state<Record<string, boolean>>({});
  let failures = $state<Record<string, LocalizedError | string>>({});
  let muteSeconds = $state(8 * 3600);
  let now = $state(Math.floor(Date.now() / 1000));
  let generation = 0;
  let messageRows = $state.raw<StoredMessage[]>([]);
  let messageLoading = $state(false);
  let messageError = $state<LocalizedError | string | null>(null);
  let messageCapped = $state(false);
  let messageRequest = 0;
  let messageRetry = $state(0);
  let messageScope = $state<{ account: string; requestKey: string | number; request: number; labels: string; chatIds: string | null;
    label: string; labelled: boolean; query: string; unread: boolean; mentions: boolean; muted: boolean; archived: boolean } | null>(null);
  const assigned = $derived(labels === null ? {} : labelsByChat);
  const shown = $derived(inboxChats(chats, filters, assigned, chatLabelOf, now));
  const filtered = $derived(filters.query.trim() || filters.label || kinds.some(([kind]) => filters[kind]));
  const messageMode = $derived(filters.labelled || !!filters.label);
  const messageLabelIds = $derived(filters.label ? [filters.label] : filters.labelled ? (labels ?? []).map((label) => label.id) : []);
  const messageTriageActive = $derived(filters.unread || filters.mentions || filters.muted || filters.archived);
  const messageChatIds = $derived(messageTriageActive
    ? inboxChats(chats, { ...filters, labelled: false, label: "", query: "" }, assigned, chatLabelOf, now).map((chat) => chat.chat)
    : null);
  const messageChatScope = $derived(messageChatIds === null ? null : JSON.stringify(messageChatIds));
  const messageScopeCurrent = $derived(messageScope !== null && messageScope.account === account && messageScope.requestKey === requestKey
    && messageScope.request === messageRequest && messageScope.labels === JSON.stringify(messageLabelIds) && messageScope.chatIds === messageChatScope
    && messageScope.label === filters.label && messageScope.labelled === filters.labelled && messageScope.query === filters.query
    && messageScope.unread === filters.unread && messageScope.mentions === filters.mentions
    && messageScope.muted === filters.muted && messageScope.archived === filters.archived);
  const visibleMessageRows = $derived(messageScopeCurrent ? messageRows : []);
  const visibleMessageLoading = $derived(messageScopeCurrent && messageLoading);
  const visibleMessageError = $derived(messageScopeCurrent ? messageError : null);
  const visibleMessageCapped = $derived(messageScopeCurrent && messageCapped);
  const messageLabelsById = $derived.by(() => {
    const result: Record<string, string[]> = {};
    for (const association of messageLabels) (result[JSON.stringify([association.chat, association.message_id])] ??= []).push(association.label_id);
    return result;
  });
  const filteredMessages = $derived(visibleMessageRows);

  $effect(() => {
    account; requestKey; connected;
    ++generation;
    busy = {};
    failures = {};
    return () => { ++generation; };
  });
  $effect(() => { account; requestKey; filters = initialFilters ? { ...initialFilters } : emptyFilters(); });
  $effect(() => {
    const owner = account, requestScope = requestKey, isConnected = connected, scope = generation;
    const associations = messageLabels;
    const mode = messageMode, ids = [...messageLabelIds], allowedScope = messageChatScope;
    const allowedChats = allowedScope === null ? null : JSON.parse(allowedScope) as string[];
    const hasLabels = labels !== null, isLoadingLabels = labelsLoading;
    const label = filters.label, labelled = filters.labelled, query = filters.query;
    const unread = filters.unread, mentions = filters.mentions, muted = filters.muted, archived = filters.archived;
    const request = ++messageRequest;
    void associations; void isConnected; void messageRetry;
    untrack(() => {
      if (!mode || !owner || !hasLabels || isLoadingLabels || ids.length === 0) {
        messageScope = null; messageRows = []; messageLoading = false; messageError = null; messageCapped = false;
        return;
      }
      messageScope = { account: owner, requestKey: requestScope, request, labels: JSON.stringify(ids),
        chatIds: allowedChats === null ? null : JSON.stringify(allowedChats), label, labelled, query, unread, mentions, muted, archived };
      void loadMessages(owner, requestScope, scope, request, ids, allowedChats, {
        label, labelled, query, unread, mentions, muted, archived,
      });
    });
  });
  $effect(() => { const current = { ...filters }; untrack(() => onfilterschange?.(current)); });
  $effect(() => {
    const timer = setInterval(() => { now = Math.floor(Date.now() / 1000); }, 1000);
    return () => clearInterval(timer);
  });

  function messageLabelNames(message: StoredMessage) {
    const ids = [...(messageLabelsById[JSON.stringify([message.chat, message.id])] ?? [])];
    if (filters.label && !ids.includes(filters.label)) ids.unshift(filters.label);
    return ids.map((id) => labels?.find((label) => label.id === id)).filter((label) => label !== undefined);
  }

  async function act(chat: ChatSummary, action: InboxAction) {
    if (!account || !connected || loading || busy[chat.chat] || action.kind === "label" && (!labels || !labelsWritable || labelsLoading)) return;
    const owner = account, revision = generation;
    busy = { ...busy, [chat.chat]: true };
    failures = { ...failures, [chat.chat]: "" };
    const current = () => owner === account && revision === generation && connected;
    try { await onaction(owner, chat.chat, action); }
    catch (failure) { if (current()) failures = { ...failures, [chat.chat]: normalizeError(failure) }; }
    finally { if (current()) busy = { ...busy, [chat.chat]: false }; }
  }

  async function loadMessages(owner: string, requestScope: string | number, scope: number, request: number, ids: string[], chatIds: string[] | null,
    filtersAtStart: Pick<InboxFilters, "label" | "labelled" | "query" | "unread" | "mentions" | "muted" | "archived">) {
    const current = () => owner === account && requestScope === requestKey && scope === generation && request === messageRequest
      && filters.label === filtersAtStart.label && filters.labelled === filtersAtStart.labelled && filters.query === filtersAtStart.query
      && filters.unread === filtersAtStart.unread && filters.mentions === filtersAtStart.mentions
      && filters.muted === filtersAtStart.muted && filters.archived === filtersAtStart.archived;
    messageRows = []; messageLoading = true; messageError = null; messageCapped = false;
    try {
      const rows = new Map<string, StoredMessage>();
      let capped = false;
      for (let offset = 0; offset < ids.length; offset += 50) {
        if (!current()) return;
        const batch = await invoke<StoredMessage[]>("labelled_messages", {
          accountId: owner, labelIds: ids.slice(offset, offset + 50), chat: null, chatIds, query: filtersAtStart.query.trim(), limit: 500,
        });
        if (!current()) return;
        capped ||= batch.length === 500;
        for (const row of batch) rows.set(JSON.stringify([row.chat, row.id]), row);
      }
      const ordered = [...rows.values()].sort((left, right) => compareMessages(right, left)
        || (left.chat === right.chat ? 0 : left.chat < right.chat ? -1 : 1));
      capped ||= ordered.length > 500;
      if (current()) { messageRows = ordered.slice(0, 500); messageCapped = capped; }
    } catch (failure) {
      if (current()) messageError = normalizeError(failure);
    } finally {
      if (current()) messageLoading = false;
    }
  }
</script>

<section class="inbox" aria-label={t("chat.unified_inbox")}>
  <header><h2>{messageMode ? t("chat.inbox_labelled_messages") : t("chat.inbox")}</h2><span class="connection" role="status">{connected ? t("settings.connected") : t("chat.inbox_offline")}</span></header>
  {#if syncPending > 0}
    <div class="sync" role="status">
      <span>{connected ? t("chat.catching_up") : t("chat.catch_up_paused")}: {t("chat.catch_up_counts", { applied: Math.min(syncPending, Math.max(0, syncApplied)), count: syncPending })}</span>
      <progress aria-label={t("chat.catch_up_label")} max={syncPending} value={Math.min(syncPending, Math.max(0, syncApplied))}></progress>
    </div>
  {/if}
  {#if historyPercent !== null}
    <div class="sync" role="status"><span>{t("chat.history_progress", { percent: formatNumber(Math.min(100, Math.max(0, historyPercent)) / 100, { style: "percent", maximumFractionDigits: 0 }) })}</span>
      <progress aria-label={t("chat.history_sync")} max="100" value={Math.min(100, Math.max(0, historyPercent))}></progress></div>
  {/if}
  {#if backfill}<p class="status" role="status">{t("chat.history_backfill", { done: backfill.done, count: backfill.total })}</p>{/if}
  {#if finalizing}<p class="status" role="status">{t("chat.sync_finishing")}</p>{/if}
  <div class="filters">
    <input class="search" type="search" dir="auto" aria-label={t(messageMode ? "chat.inbox_search_messages" : "chat.inbox_search")}
      placeholder={t(messageMode ? "chat.inbox_search_messages" : "chat.search")} bind:value={filters.query} />
    <fieldset><legend>{t("chat.match_filters")}</legend>
      {#each kinds as [kind, label]}
        <label class="pill" class:on={filters[kind]}>
          <input type="checkbox" bind:checked={filters[kind]} disabled={kind === "labelled" && labels === null} />
          <span>{t(label)}</span>
        </label>
      {/each}
    </fieldset>
    <label class="choice">{t("labels.label")} <select bind:value={filters.label} disabled={labels === null}>
      <option value="">{t("labels.any")}</option>{#each labels ?? [] as label (label.id)}<option value={label.id}><bdi>{label.name}</bdi></option>{/each}
    </select></label>
    <label class="choice">{t("chat.mute_duration")} <select bind:value={muteSeconds}>
      <option value={8 * 3600}>{t("chat.mute_eight_hours")}</option><option value={7 * 86400}>{t("chat.mute_week")}</option><option value={-1}>{t("ui.always")}</option>
    </select></label>
    {#if filtered}<button onclick={() => (filters = emptyFilters())}>{t("chat.filters_clear")}</button>{/if}
  </div>
  {#if labelsLoading}<p class="status" role="status">{t("labels.loading")}</p>
  {:else if labels === null && !labelsError}<p class="status">{t("labels.unavailable_account")}</p>
  {:else if !labelsWritable}<p class="status">{t("labels.read_only")}</p>{/if}
  {#if labels !== null && !labelsComplete}<p class="status">{t("labels.cached_hint")}</p>{/if}
  {#if labelsError}<p class="error" role="alert">{t("labels.load_failed", { error: normalizeError(labelsError).message })}</p>{/if}
  {#if error}<div class="error" role="alert">{error}{#if onretry}<button onclick={onretry} disabled={loading}>{t("ui.retry")}</button>{/if}</div>{/if}
  {#if !account}<p class="status" role="status">{t("chat.inbox_select_account")}</p>
  {:else}
    {#if loading}<p class="status" role="status">{t("chat.inbox_loading")}</p>{/if}
    {#if labelsLoading && (filters.labelled || filters.label)}<p class="status" role="status">{t("chat.inbox_wait_labels")}</p>
    {:else if messageMode}
      {#if visibleMessageError}<div class="error" role="alert">{visibleMessageError}<button onclick={() => (messageRetry++)} disabled={visibleMessageLoading}>{t("ui.retry")}</button></div>{/if}
      {#if visibleMessageCapped}<p class="status" role="status">{t("chat.inbox_messages_capped", { count: 500 })}</p>{/if}
      {#if visibleMessageLoading && filteredMessages.length === 0}<p class="status" role="status">{t("chat.inbox_messages_loading")}</p>{/if}
      {#if filteredMessages.length === 0 && !visibleMessageLoading && !visibleMessageError}
        <p class="status" role="status">{t("chat.inbox_messages_empty")}</p>
      {/if}
      <ul class="message-results">
        {#each filteredMessages as message (JSON.stringify([message.chat, message.id]))}
          {@const chat = chats.find((row) => row.chat === message.chat)}
          {@const name = chat ? chatLabelOf(chat) : message.chat}
          {@const categories = chat ? inboxCategories(chat, assigned[chat.chat] ?? [], now) : null}
          {@const chips = messageLabelNames(message)}
          <li>
            <button class="chat message-entry" onclick={() => onopenmessage?.(message.chat, message.id)} disabled={!onopenmessage}>
              <Avatar src={avatarOf(message.chat)} label={name} seed={message.chat} cls="inbox-avatar" />
              <span class="body"><span class="title"><bdi>{name}</bdi></span><span class="preview">{message.text || t("chat.no_messages")}</span></span>
              <time>{formatTime(message.timestamp)}</time>
            </button>
            <div class="badges">
              {#if categories?.unread}<span>{chat?.unread_count ? t("chat.unread_count", { count: chat.unread_count }) : t("chat.marked_unread")}</span>{/if}
              {#if categories?.mentions}<span>{chat?.mention_count} {t("chat.mentions")}</span>{/if}
              {#if categories?.muted}<span>{t("chat.muted")}</span>{/if}{#if categories?.archived}<span>{t("chat.archived")}</span>{/if}
              {#each chips as label (label.id)}
                <span class="label message-label"><bdi>{label.name}</bdi></span>
              {/each}
            </div>
          </li>
        {/each}
      </ul>
    {:else}
      {#if shown.length === 0 && !loading && !error}
        <p class="status" role="status">{filtered ? t("chat.inbox_no_matches") : chats.length === 0 ? t("chat.inbox_empty_account") : t("chat.inbox_empty")}</p>
      {/if}
      <ul>
        {#each shown as chat (chat.chat)}
          {@const name = chatLabelOf(chat)}
          {@const categories = inboxCategories(chat, assigned[chat.chat] ?? [], now)}
          {@const disabled = !connected || loading || !!busy[chat.chat]}
          <li>
            <button class="chat" onclick={() => onopen(chat.chat)} disabled={loading}>
              <Avatar src={avatarOf(chat.chat)} label={name} seed={chat.chat} cls="inbox-avatar" />
              <span class="body"><span class="title"><bdi>{name}</bdi></span><span class="preview">{previewTextOf(chat) || t("chat.no_messages")}</span></span>
              {#if chat.last_message_at > 0}<time>{formatTime(chat.last_message_at)}</time>{/if}
            </button>
            <div class="badges">
              {#if categories.unread}<span>{chat.unread_count > 0 ? t("chat.unread_count", { count: chat.unread_count }) : t("chat.marked_unread")}</span>{/if}
              {#if categories.mentions}<button class="mention" onclick={() => onopen(chat.chat, true)} disabled={loading}>{chat.mention_count} {chat.mention_count === 1 ? "mention" : "mentions"}</button>{/if}
              {#if categories.muted}<span>{t("chat.muted")}</span>{/if}{#if categories.archived}<span>{t("chat.archived")}</span>{/if}
              {#each (labels ?? []).filter((label) => assigned[chat.chat]?.includes(label.id)) as label (label.id)}
                <span class="label">{label.name}{#if labelsWritable}<button aria-label={t("labels.remove_from_chat", { label: label.name, chat: name })} disabled={disabled}
                  onclick={() => act(chat, { kind: "label", label: label.id, applied: false })}>×</button>{/if}</span>
              {/each}
            </div>
            <div class="actions" aria-label={t("chat.actions_for", { name })}>
              <button disabled={disabled} onclick={() => act(chat, { kind: "read", read: categories.unread })}>{categories.unread ? t("chat.mark_read") : t("chat.mark_unread")}</button>
              <button disabled={disabled} onclick={() => act(chat, { kind: "archive", archived: !chat.archived })}>{chat.archived ? t("chat.unarchive") : t("chat.archive")}</button>
              <button disabled={disabled} onclick={() => act(chat, { kind: "mute", seconds: categories.muted ? 0 : muteSeconds })}>{categories.muted ? t("chat.unmute") : t("chat.mute")}</button>
              <select aria-label={t("labels.apply_to_chat", { name })} disabled={disabled || labels === null || !labelsWritable || labelsLoading}
                value="" onchange={(event) => { const label = event.currentTarget.value; event.currentTarget.value = ""; if (label) void act(chat, { kind: "label", label, applied: true }); }}>
                <option value="">{t("labels.apply_one")}</option>
                {#each appliableLabels(labels ?? [], assigned[chat.chat]) as label (label.id)}<option value={label.id}>{label.name}</option>{/each}
              </select>
              {#if busy[chat.chat]}<span role="status">{t("ui.updating")}</span>{/if}
            </div>
            {#if failures[chat.chat]}<p class="error" role="alert">{failures[chat.chat]}</p>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .inbox { min-width: 0; min-height: 0; overflow: auto; padding: 20px; color: var(--text); background: var(--bg); }
  header { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  h2 { margin: 0; font-size: 1.25rem; }
  .connection, .status, .sync { color: var(--muted); font-size: 0.8125rem; }
  .sync { display: grid; gap: 5px; margin: 12px 0; }
  progress { width: 100%; height: 7px; accent-color: var(--accent); }
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; margin: 18px 0 12px; }
  .search { flex: 1 1 100%; min-width: 0; }
  input, select { padding: 7px 9px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: inherit; font: inherit; }
  fieldset { display: flex; flex-wrap: wrap; gap: 8px; margin: 0; padding: 0; border: 0; }
  legend { margin-bottom: 6px; color: var(--muted); font-size: 0.75rem; }
  fieldset label, .choice { display: flex; align-items: center; gap: 5px; font-size: 0.8125rem; }
  .filters > .choice:first-of-type { margin-inline-start: auto; }
  .choice select { border: 0; border-radius: 999px; background: var(--surface); color: var(--muted); padding: 5px 12px; font-size: 0.8125rem; }
  .choice select:hover { background: var(--raised); }
  .pill { position: relative; padding: 5px 12px; border-radius: 999px; background: var(--surface); color: var(--muted); cursor: pointer; }
  .pill:hover { background: var(--raised); }
  .pill.on { background: var(--accent-soft); color: var(--accent); }
  .pill input { position: absolute; inset: 0; margin: 0; padding: 0; opacity: 0; cursor: pointer; }
  .pill:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .pill:has(input:disabled) { opacity: 0.55; cursor: default; }
  button { padding: 6px 9px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: inherit; font: inherit; font-size: 0.75rem; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--raised-2); }
  button:disabled, select:disabled { opacity: 0.55; cursor: default; }
  ul { margin: 0; padding: 0; list-style: none; }
  li { padding: 14px 0; border-bottom: 1px solid var(--line); }
  .chat { display: flex; width: 100%; align-items: center; gap: 10px; padding: 0; border: 0; background: transparent; text-align: start; }
  .body { flex: 1; min-width: 0; display: grid; gap: 4px; }
  .title { font-size: 0.875rem; font-weight: 600; overflow-wrap: anywhere; }
  .preview { color: var(--muted); font-size: 0.75rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  time { flex: none; color: var(--muted); font-size: 0.6875rem; }
  .badges, .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 10px; }
  .badges { color: var(--muted); font-size: 0.75rem; }
  .badges > span, .mention { padding: 3px 7px; border: 0; border-radius: var(--radius-sm); background: var(--surface); }
  .mention { color: var(--accent-text); }
  .label { display: inline-flex; align-items: center; gap: 5px; }
  .label button { padding: 0 3px; border: 0; background: transparent; font-size: 0.9375rem; }
  .actions select { max-width: 100%; font-size: 0.75rem; }
  .actions span { color: var(--muted); font-size: 0.75rem; }
  .error { color: var(--danger); font-size: 0.8125rem; overflow-wrap: anywhere; }
  .error button { margin-inline-start: 10px; }
  :global(.inbox-avatar) { width: 36px; height: 36px; flex: none; border-radius: 50%; object-fit: cover; display: grid; place-items: center; background: hsl(var(--hue, 0) 25% 35%); color: white; font-size: 0.875rem; }
  @media (max-width: 480px) { .inbox { padding: 12px; } time { max-width: 80px; text-align: end; } }
</style>
