<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { locale } from "$lib/i18n/locale.svelte";
  import { channels } from "$lib/state/channels.svelte";
  import type { ChannelSummary } from "$lib/utils/wire";
  import Avatar from "$lib/ui/Avatar.svelte";
  import Button from "$lib/ui/Button.svelte";
  import ChannelActions from "$lib/chat/ChannelActions.svelte";

  let {
    account,
    generation,
    connected,
    onOpen,
  }: {
    account: string | null;
    generation: number;
    connected: boolean;
    onOpen: (jid: string) => void;
  } = $props();

  type Tab = "following" | "favorites" | "discover";
  let tab = $state<Tab>("following");
  let query = $state("");
  let opening = $state("");
  const tabKeys = ["following", "favorites", "discover"] as const;
  const following = $derived(channels.view?.channels.filter((channel) => channel.followed) ?? []);
  const favoriteChannels = $derived(channels.view?.channels.filter((channel) => channel.favorite) ?? []);

  $effect(() => { void channels.activate(account, generation, connected); });

  async function lookup() {
    if (!account) return;
    await channels.lookup(account, generation, query);
  }

  function action(channel: ChannelSummary, type: "follow" | "unfollow" | "mute", muted = false) {
    if (!account) return;
    if (type === "follow") void channels.follow(account, generation, channel.jid);
    else if (type === "unfollow") void channels.unfollow(account, generation, channel.jid);
    else void channels.setMuted(account, generation, channel.jid, muted);
  }

  function favorite(channel: ChannelSummary, value: boolean) {
    if (account) void channels.setFavorite(account, generation, channel.jid, value);
  }
  async function open(channel: ChannelSummary) {
    if (!account || !channel.followed || opening) return;
    const owner = account, scope = generation;
    opening = channel.jid;
    if (connected) await channels.pageMessages(owner, scope, channel.jid, 50, true);
    if (owner === account && scope === generation) onOpen(channel.jid);
    if (opening === channel.jid) opening = "";
  }

  function tabKeydown(event: KeyboardEvent, index: number) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const delta = (event.key === "ArrowRight") === (locale.dir !== "rtl") ? 1 : -1;
    const next = (index + delta + tabKeys.length) % tabKeys.length;
    tab = tabKeys[next];
    requestAnimationFrame(() => document.querySelectorAll<HTMLButtonElement>(".channels .tabs button")[next]?.focus());
  }
</script>

<section class="channels" aria-label={t("channels.title")}>
  <header>
    <div><h1>{t("channels.title")}</h1>
      {#if channels.refreshing}<small role="status">{t("channels.refreshing")}</small>
      {:else if channels.view?.synced_at !== null && channels.view}{#if connected}<small>{t("channels.synced")}</small>{:else}<small>{t("channels.offline_cached")}</small>{/if}{/if}
    </div>
    <Button variant="icon" icon="repeat" title={t("channels.refresh")} aria-label={t("channels.refresh")}
      disabled={!account || !connected || channels.refreshing} onclick={() => account && void channels.refresh(account, generation)} />
  </header>
  <div class="tabs" role="tablist" aria-label={t("channels.title")}>
    <Button id="channel-tab-following" variant="chip" selected={tab === "following"} tabindex={tab === "following" ? 0 : -1}
      aria-controls="channel-following" onkeydown={(event: KeyboardEvent) => tabKeydown(event, 0)} onclick={() => (tab = "following")}>{t("channels.following")}</Button>
    <Button id="channel-tab-favorites" variant="chip" selected={tab === "favorites"} tabindex={tab === "favorites" ? 0 : -1}
      aria-controls="channel-favorites" onkeydown={(event: KeyboardEvent) => tabKeydown(event, 1)} onclick={() => (tab = "favorites")}>{t("channels.favorites")}</Button>
    <Button id="channel-tab-discover" variant="chip" selected={tab === "discover"} tabindex={tab === "discover" ? 0 : -1}
      aria-controls="channel-discover" onkeydown={(event: KeyboardEvent) => tabKeydown(event, 2)} onclick={() => (tab = "discover")}>{t("channels.discover")}</Button>
  </div>
  {#if channels.error}<p class="error" role="alert">{channels.error.message}</p>{/if}
  {#if !account}
    <p class="empty" role="status">{t("channels.empty")}</p>
  {:else if tab === "discover"}
    <div id="channel-discover" role="tabpanel" aria-labelledby="channel-tab-discover" tabindex="0" class="discover">
      <div class="lookup-form">
        <label for="channel-jid">{t("channels.search_jid")}</label>
        <div class="lookup">
          <input id="channel-jid" bind:value={query} placeholder={t("channels.lookup_hint")} autocomplete="off" spellcheck="false"
            onkeydown={(event: KeyboardEvent) => { if (event.key === "Enter") { event.preventDefault(); void lookup(); } }} />
          <Button variant="primary" disabled={!connected || channels.lookingUp || !query.trim()} onclick={() => void lookup()}>
            {channels.lookingUp ? t("channels.loading") : t("channels.lookup")}
          </Button>
        </div>
      </div>
      {#if channels.lookupError}<p class="error" role="alert">{channels.lookupError === "channels.invalid_target" ? t(channels.lookupError) : channels.lookupError}</p>{/if}
      {#if channels.preview}
        <article class="preview">
          <ChannelActions channel={channels.preview} busy={channels.busy === channels.preview.jid} disabled={!connected}
            onfollow={() => action(channels.preview!, "follow")} onunfollow={() => action(channels.preview!, "unfollow")}
            onmute={(muted) => action(channels.preview!, "mute", muted)} onfavorite={(value) => favorite(channels.preview!, value)} />
          <Button variant="primary" cls="open-channel" disabled={!channels.preview.followed || opening !== ""} onclick={() => void open(channels.preview!)}>{t("channels.open_channel")}</Button>
        </article>
      {/if}
    </div>
  {:else}
    {@const rows = tab === "favorites" ? favoriteChannels : following}
    <div id={tab === "favorites" ? "channel-favorites" : "channel-following"}
      aria-labelledby={tab === "favorites" ? "channel-tab-favorites" : "channel-tab-following"} role="tabpanel" tabindex="0" class="list">
      {#if channels.loading && !channels.view}
        <p class="empty" role="status">{t("channels.loading")}</p>
      {:else if rows.length === 0}
        <p class="empty">{t(tab === "favorites" ? "channels.favorites_empty" : "channels.empty")}</p>
      {:else}
        {#each rows as channel (channel.jid)}
          <article class="row">
            <button type="button" class="open" disabled={!channel.followed || opening !== ""} onclick={() => void open(channel)}>
              <Avatar src={channel.picture_url} label={channel.name || channel.jid} seed={channel.jid} cls="avatar" />
              <span class="summary"><strong>{channel.name || channel.jid}</strong><small>{channel.description || channel.jid}</small></span>
            </button>
            <ChannelActions compact channel={channel} busy={channels.busy === channel.jid} disabled={!connected}
              onfollow={() => action(channel, "follow")} onunfollow={() => action(channel, "unfollow")}
              onmute={(muted) => action(channel, "mute", muted)} onfavorite={(value) => favorite(channel, value)} />
          </article>
        {/each}
      {/if}
    </div>
  {/if}
</section>

<style>
  .channels { display: flex; flex-direction: column; min-width: 0; height: 100%; overflow: hidden; background: var(--bg); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 16px 20px 10px; border-bottom: 1px solid var(--line); }
  h1 { margin: 0; font-size: 1.25rem; }
  header small { color: var(--muted); font-size: 0.75rem; }
  .tabs { display: flex; flex-wrap: wrap; gap: 8px; padding: 12px 16px; border-bottom: 1px solid var(--line); }
  .list, .discover { flex: 1; min-height: 0; overflow-y: auto; padding: 12px; }
  .row { display: flex; align-items: center; gap: 8px; padding: 8px; border-bottom: 1px solid var(--line); }
  .open { display: flex; align-items: center; gap: 10px; flex: 1; min-width: 0; padding: 4px; text-align: start; border: 0; background: transparent; color: inherit; font: inherit; cursor: pointer; }
  .open:disabled { cursor: default; opacity: 0.65; }
  :global(.avatar) { width: 38px; height: 38px; flex: none; border-radius: 50%; }
  .summary { display: flex; flex-direction: column; min-width: 0; gap: 3px; }
  .summary strong, .summary small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .summary small { color: var(--muted); font-size: 0.75rem; }
  .lookup-form { display: grid; gap: 8px; max-width: 620px; }
  .discover label { font-size: 0.875rem; font-weight: 600; }
  .lookup { display: flex; gap: 8px; }
  .lookup input { flex: 1; min-width: 0; padding: 8px 12px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--surface); color: var(--text); font: inherit; }
  .preview { max-width: 680px; margin-top: 18px; padding: 8px 0; border-top: 1px solid var(--line); }
  .preview > :global(.btn) { margin-inline-start: 58px; }
  .empty { padding: 24px 12px; color: var(--muted); text-align: center; }
  .error { padding: 8px 12px; color: var(--danger); overflow-wrap: anywhere; }
  @media (max-width: 620px) { .row { flex-wrap: wrap; } .row .open { flex-basis: 100%; } }
</style>
