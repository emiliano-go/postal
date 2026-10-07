<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t, formatNumber } from "$lib/i18n/localizer";
  import Icon from "$lib/ui/Icon.svelte";
  import Button from "$lib/ui/Button.svelte";
  import type { CallRecord } from "$lib/utils/wire";

  let {
    account,
    generation,
    connected,
    refreshKey = 0,
    labelFor,
    onopen,
  }: {
    account: string | null;
    generation: number;
    connected: boolean;
    refreshKey?: number;
    labelFor: (jid: string) => string;
    onopen: (chat: string) => void | Promise<void>;
  } = $props();

  let records = $state<CallRecord[]>([]);
  let recordsFor = $state<{ account: string; generation: number } | null>(null);
  let loading = $state(false);
  let error = $state<LocalizedError | null>(null);
  let request = 0;
  const visibleRecords = $derived(recordsFor !== null && recordsFor.account === account && recordsFor.generation === generation ? records : []);

  $effect(() => {
    void account; void generation; void refreshKey;
    untrack(() => { void refresh(); });
  });

  async function refresh() {
    const owner = account, epoch = generation, currentRequest = ++request;
    const current = () => owner === account && epoch === generation && currentRequest === request;
    error = null;
    if (!owner) { records = []; recordsFor = null; loading = false; return; }
    if (recordsFor?.account !== owner || recordsFor.generation !== epoch) {
      records = [];
      recordsFor = { account: owner, generation: epoch };
    }
    loading = true;
    try {
      const result = await invoke<CallRecord[]>("call_history", { account: owner, limit: 200 });
      if (current()) records = result;
    } catch (failure) {
      if (current()) error = normalizeError(failure);
    } finally {
      if (current()) loading = false;
    }
  }

  function direction(record: CallRecord) {
    if (record.outcome === "missed") return t("calls.missed");
    return t(record.from_me ? "calls.outgoing" : "calls.incoming");
  }

  function callType(record: CallRecord) {
    if (record.is_video === true) return t("calls.video");
    if (record.is_video === false) return t("calls.voice");
    return t("calls.type_unknown");
  }

  function outcome(record: CallRecord) {
    return t(`calls.outcome_${record.outcome}`);
  }

  function contact(record: CallRecord) {
    const jid = record.chat ?? record.peer_jid ?? record.group_jid;
    return jid ? labelFor(jid) : t("calls.unknown_contact");
  }
</script>

<section class="calls" aria-label={t("calls.title")} aria-busy={loading}>
  <header>
    <div><h1>{t("calls.title")}</h1><p>{t("calls.description")}</p></div>
    <Button variant="icon" icon="repeat" title={t("calls.refresh")} aria-label={t("calls.refresh")} disabled={loading || !account} onclick={() => void refresh()} />
  </header>
  {#if !connected && account}<p class="offline" role="status">{t("calls.offline_cached")}</p>{/if}
  {#if error}<div class="error" role="alert"><p>{error.message}</p>
    {#if error.diagnostic}<details><summary>{t("error.technical_details")}</summary><pre dir="auto">{error.diagnostic}</pre></details>{/if}
  </div>{/if}
  {#if loading && visibleRecords.length === 0}
    <p class="status" role="status">{t("calls.loading")}</p>
  {:else if !account}
    <p class="empty" role="status">{t("calls.choose_account")}</p>
  {:else if !visibleRecords.length && !error}
    <p class="empty" role="status">{t("calls.empty")}</p>
  {:else if visibleRecords.length}
    <div class="list">
      <p class="undated">{t("calls.undated_hint")}</p>
      <ol>
        {#each visibleRecords as record (record.call_id)}
          {@const chat = record.chat ?? record.peer_jid ?? record.group_jid}
          <li>
            <button class="entry" type="button" disabled={!chat || loading} onclick={() => chat && onopen(chat)}>
              <span class="kind" aria-hidden="true">
                {#if record.is_video === true}<Icon name="video" size={18} />
                {:else if record.is_video === false}<Icon name="volume" size={18} />
                {:else}<span>?</span>{/if}
              </span>
              <span class="body">
                <strong><bdi dir="auto">{contact(record)}</bdi></strong>
                <span class="meta"><span class:missed={record.outcome === "missed"}>{direction(record)}</span><span>{callType(record)}</span>
                  {#if record.outcome !== "missed"}<span>{outcome(record)}</span>{/if}
                </span>
                <span class="uncertain">{t("calls.date_unavailable")} · {t("calls.duration_unavailable")}</span>
              </span>
            </button>
          </li>
        {/each}
      </ol>
      {#if loading}<p class="status" role="status">{t("calls.refreshing", { count: formatNumber(visibleRecords.length) })}</p>{/if}
    </div>
  {/if}
</section>

<style>
  .calls { min-width: 0; min-height: 0; flex: 1; display: flex; flex-direction: column; padding: 20px clamp(16px, 4vw, 48px); overflow: hidden; color: var(--text); }
  header { display: flex; align-items: start; justify-content: space-between; gap: 16px; }
  h1 { margin: 0; font-size: 1.25rem; }
  header p { margin: 6px 0 16px; color: var(--muted); font-size: .8125rem; }
  .list { min-height: 0; overflow: auto; }
  ol { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  .entry { width: 100%; display: flex; align-items: center; gap: 12px; padding: 12px; text-align: start; border: 1px solid transparent; border-radius: var(--radius-sm); background: var(--surface); color: inherit; font: inherit; cursor: pointer; }
  .entry:hover:not(:disabled) { border-color: var(--line); background: var(--raised); }
  .entry:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .entry:disabled { cursor: default; opacity: .7; }
  .kind { display: grid; place-items: center; width: 38px; height: 38px; flex: none; border-radius: 50%; background: var(--raised); color: var(--accent); }
  .body { min-width: 0; flex: 1; display: grid; gap: 5px; }
  .body strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: .875rem; }
  .meta { display: flex; flex-wrap: wrap; gap: 8px; color: var(--muted); font-size: .75rem; }
  .missed { color: var(--danger); }
  .uncertain, .undated { color: var(--muted); font-size: .75rem; }
  .undated { margin: 0 0 10px; }
  .empty, .status, .offline, .error { color: var(--muted); font-size: .8125rem; }
  .error { color: var(--danger); font-size: .8125rem; overflow-wrap: anywhere; }
  .error p { margin: 0; }
  .error pre { color: var(--muted); white-space: pre-wrap; }
  @media (max-width: 520px) { .calls { padding-inline: 12px; } .entry { gap: 8px; padding: 10px 8px; } .kind { width: 32px; height: 32px; } }
</style>
