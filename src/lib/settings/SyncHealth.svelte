<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "$lib/utils/ipc";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import { t, formatDate } from "$lib/i18n/localizer";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import type { ServiceEvent, SyncCollection, SyncHealthView, SyncMode, SyncRepairReport, SyncStatus } from "$lib/utils/wire";

  let { account, connected }: { account: string | null; connected: boolean } = $props();
  let health = $state<SyncHealthView | null>(null);
  let report = $state<SyncRepairReport | null>(null);
  let loading = $state(false);
  let busy = $state(false);
  let error = $state<LocalizedError | string | null>(null);
  let fullTarget = $state<SyncCollection[] | null>(null);
  let revision = 0, readRequest = 0;

  const unavailable = $derived(!account || !connected || busy || loading || !!health?.busy || !!health?.automatic_running);

  function date(at: number): string { return formatDate(at / 1000, { dateStyle: "medium", timeStyle: "short" }); }
  function collection(name: SyncCollection): string { return name; }
  function status(state: SyncStatus): string { return t(`sync.status.${state}`); }

  async function refresh(clearError = false) {
    const owner = account, turn = revision, read = ++readRequest;
    if (!owner) { health = null; report = null; loading = false; return; }
    loading = true;
    if (clearError) error = null;
    try {
      const next = await invoke<SyncHealthView>("sync_health", { accountId: owner });
      if (turn !== revision || read !== readRequest || account !== owner) return;
      health = next;
      if (!busy) report = next.last_report;
    } catch (failure) {
      if (turn === revision && read === readRequest && account === owner) error = normalizeError(failure);
    } finally {
      if (turn === revision && read === readRequest && account === owner) loading = false;
    }
  }

  async function repair(collections: SyncCollection[] | null, mode: SyncMode) {
    const owner = account;
    if (!owner || unavailable) return;
    const turn = revision;
    busy = true;
    error = null;
    report = null;
    fullTarget = null;
    try {
      const next = await invoke<SyncRepairReport>("repair_sync", { accountId: owner, collections, mode });
      if (turn !== revision || account !== owner) return;
      report = next;
    } catch (failure) {
      if (turn === revision && account === owner) error = normalizeError(failure);
    } finally {
      if (turn === revision && account === owner) {
        busy = false;
        await refresh(false);
      }
    }
  }

  $effect(() => {
    void connected;
    const owner = account;
    ++revision;
    health = null;
    report = null;
    error = null;
    fullTarget = null;
    busy = loading = false;
    if (owner) void untrack(() => refresh(true));
  });

  onMount(() => {
    let removed = false;
    let unlisten: (() => void) | null = null;
    void listen<ServiceEvent>("service-event", (event) => {
      if (event.payload.kind === "syncHealthChanged") void refresh();
    }).then((stop) => { if (removed) stop(); else unlisten = stop; }).catch(() => {});
    return () => { removed = true; unlisten?.(); };
  });
</script>

<section class="sync-health" aria-label={t("sync.title")}>
  <p class="muted">{t("sync.scope")}</p>
  {#if !account || !connected}<p role="status">{t("sync.connect")}</p>{/if}
  {#if loading && !health}<p role="status">{t("sync.loading")}</p>{/if}
  {#if health}
    {#if health.automatic_running}<p role="status">{t("sync.auto_running")}</p>
    {:else if health.automatic_attempted}<p role="status">{t("sync.auto_attempted")}</p>{/if}
    {#if health.last_error}<p role="alert" class="error">{health.last_error}</p>{/if}
    {#if health.storage_error}<p role="alert" class="error">{health.storage_error}</p>{/if}
    <div class="collections">
      {#each health.collections as row (row.collection)}
        <div class="collection">
          <div class="collection-info">
            <strong><code>{collection(row.collection)}</code></strong>
            <span class:problem={row.status === "dirty" || row.status === "fatal" || row.status === "retryable"}>
              {status(row.status)}
            </span>
            {#if row.version !== null}<small>{t("sync.version", { value: row.version })}</small>{/if}
            <small>{row.last_success_at === null ? t("sync.never_repaired") : t("sync.last_success", { date: date(row.last_success_at) })}</small>
            {#if row.error}<small class="error">{row.error}</small>{/if}
          </div>
          <div class="row-actions">
            <button class="button" disabled={unavailable} onclick={() => repair([row.collection], "incremental")}>{row.status === "retryable" || row.status === "skipped" ? t("sync.retry") : t("sync.repair")}</button>
            <button class="button" disabled={unavailable} onclick={() => (fullTarget = [row.collection])}>{t("sync.full")}</button>
          </div>
        </div>
      {/each}
    </div>
    <div class="actions">
      <button class="button primary" data-setting-search-id="sync-repair-all" disabled={unavailable} onclick={() => repair(null, "incremental")}>{busy || health.busy ? t("sync.repairing") : t("sync.repair_all")}</button>
      <button class="button" disabled={unavailable} onclick={() => (fullTarget = health?.collections.map((row) => row.collection) ?? [])}>{t("sync.full_all")}</button>
      <button class="button" disabled={busy || loading} onclick={() => refresh(true)}>{t("sync.refresh")}</button>
    </div>
  {/if}
  {#if report}
    <div class="report" role="status">
      <h3>{report.automatic ? t("sync.auto_report") : t("sync.report")}</h3>
      <p>{t("sync.report_mode", { mode: report.mode === "full" ? t("sync.full") : t("sync.incremental"), date: date(report.at) })}</p>
      <ul>
        <li>{t("sync.synced")}: {report.synced.length ? report.synced.join(", ") : t("sync.none")}</li>
        <li>{t("sync.retryable")}: {report.retryable.length ? report.retryable.join(", ") : t("sync.none")}</li>
        <li>{t("sync.fatal")}: {report.fatal.length ? report.fatal.join(", ") : t("sync.none")}</li>
        <li>{t("sync.skipped")}: {report.skipped.length ? report.skipped.join(", ") : t("sync.none")}</li>
        {#if report.unreported.length}<li>{t("sync.unreported")}: {report.unreported.join(", ")}</li>{/if}
      </ul>
      {#if report.retryable.length || report.skipped.length}
        <button class="button" disabled={unavailable} onclick={() => { const value = report; if (value) void repair([...value.retryable, ...value.skipped], "incremental"); }}>{t("sync.retry_failed")}</button>
      {/if}
      {#if report.fatal.length}<p class="error">{t("sync.fatal_explain")}</p>{/if}
      {#if report.storage_error}<p class="error">{report.storage_error}</p>{/if}
      <p class="muted">{t("sync.protocol_only")}</p>
    </div>
  {/if}
  {#if error}<p role="alert" class="error">{error}</p>{/if}
  {#if error instanceof LocalizedError && error.diagnostic}<details><summary>{t("error.technical_details")}</summary><pre>{error.diagnostic}</pre></details>{/if}
</section>

{#if fullTarget}
  <ConfirmDialog label={t("sync.full_confirm_title")} title={t("sync.full_confirm_title")}
    hint={t("sync.full_warning")} onclose={() => (fullTarget = null)}>
    {#snippet actions()}
      <button class="button" onclick={() => (fullTarget = null)}>{t("ui.cancel")}</button>
      <button class="button danger" disabled={unavailable} onclick={() => { if (fullTarget) void repair(fullTarget, "full"); }}>{t("sync.full_confirm")}</button>
    {/snippet}
  </ConfirmDialog>
{/if}

<style>
  .sync-health { display: grid; gap: .75rem; }
  .sync-health p { margin: 0; }
  .muted, small { color: var(--muted); }
  .collections { display: grid; gap: .5rem; }
  .collection { display: flex; align-items: center; justify-content: space-between; gap: 1rem; padding: .75rem; border: 1px solid var(--line); border-radius: .7rem; }
  .collection-info { display: grid; gap: .2rem; min-width: 0; }
  .collection-info code { overflow-wrap: anywhere; }
  .row-actions, .actions { display: flex; gap: .4rem; flex-wrap: wrap; }
  .problem, .error { color: var(--danger); }
  .report { padding: .75rem; border: 1px solid var(--line); border-radius: .7rem; }
  .report h3 { margin: 0 0 .4rem; font-size: 1rem; }
  .report ul { margin: .4rem 0; padding-inline-start: 1.3rem; overflow-wrap: anywhere; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
  @media (max-width: 650px) { .collection { align-items: stretch; flex-direction: column; } }
</style>
