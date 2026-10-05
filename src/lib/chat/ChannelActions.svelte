<script lang="ts">
  import type { ChannelSummary } from "$lib/utils/wire";
  import { formatNumber, t } from "$lib/i18n/localizer";
  import Avatar from "$lib/ui/Avatar.svelte";
  import Button from "$lib/ui/Button.svelte";

  let {
    channel,
    busy = false,
    disabled = false,
    compact = false,
    onfollow,
    onunfollow,
    onmute,
    onfavorite,
  }: {
    channel: ChannelSummary | null;
    busy?: boolean;
    disabled?: boolean;
    compact?: boolean;
    onfollow: () => void;
    onunfollow: () => void;
    onmute: (muted: boolean) => void;
    onfavorite: (favorite: boolean) => void;
  } = $props();
</script>

{#if channel}
  {#if compact}
    <div class="actions" role="group" aria-label={t("channels.header_details")}>
      <Button variant="icon" icon="star" pressed={channel.favorite} aria-label={t(channel.favorite ? "channels.unfavorite" : "channels.favorite")}
        title={t("channels.favorite_local")} disabled={busy} onclick={() => onfavorite(!channel.favorite)} />
      {#if channel.followed}
        <Button variant="ghost" pressed={channel.muted} disabled={disabled || busy}
          onclick={() => onmute(!channel.muted)}>
          {t(channel.muted ? "channels.unmute" : "channels.mute")}
        </Button>
        <Button variant="ghost" cls="unfollow-channel" aria-label={t("channels.unfollow")} disabled={disabled || busy} onclick={onunfollow}>{t("channels.unfollow")}</Button>
      {:else}
        <Button variant="primary" disabled={disabled || busy} onclick={onfollow}>{t("channels.follow")}</Button>
      {/if}
    </div>
  {:else}
    <section class="channel-actions" aria-label={t("channels.header_details")}>
      <Avatar src={channel.picture_url} label={channel.name || channel.jid} seed={channel.jid} cls="channel-avatar" />
      <div class="details">
        <strong>{channel.name || channel.jid}</strong>
        {#if channel.description}<p>{channel.description}</p>{/if}
        <span>{t("channels.subscribers", { count: formatNumber(channel.subscriber_count) })}</span>
      </div>
      <div class="actions">
        <Button variant="icon" icon="star" pressed={channel.favorite} aria-label={t(channel.favorite ? "channels.unfavorite" : "channels.favorite")}
          title={t("channels.favorite_local")} disabled={busy} onclick={() => onfavorite(!channel.favorite)} />
        <Button variant={channel.followed ? "ghost" : "primary"} cls="unfollow-channel" aria-label={t(channel.followed ? "channels.unfollow" : "channels.follow")} disabled={disabled || busy}
          onclick={channel.followed ? onunfollow : onfollow}>
          {t(channel.followed ? "channels.unfollow" : "channels.follow")}
        </Button>
        {#if channel.followed}
          <Button variant="ghost" pressed={channel.muted} disabled={disabled || busy}
            onclick={() => onmute(!channel.muted)}>
            {t(channel.muted ? "channels.unmute" : "channels.mute")}
          </Button>
        {/if}
      </div>
    </section>
  {/if}
{/if}

<style>
  .channel-actions { display: flex; align-items: center; gap: 12px; min-width: 0; padding: 12px 16px; }
  :global(.channel-avatar) { width: 42px; height: 42px; flex: none; border-radius: 50%; }
  .details { min-width: 0; flex: 1; }
  .details strong { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .details p { margin: 3px 0; color: var(--muted); font-size: 0.8125rem; white-space: pre-wrap; overflow-wrap: anywhere; }
  .details span { color: var(--muted); font-size: 0.75rem; }
  .actions { display: flex; flex-wrap: wrap; justify-content: end; gap: 6px; }
  @media (max-width: 620px) { .channel-actions:not(.compact) { align-items: flex-start; flex-wrap: wrap; } .actions { margin-inline-start: auto; } }
</style>
