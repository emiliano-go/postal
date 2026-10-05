<script lang="ts">
  import { onMount } from "svelte";
  import Settings, { type Section, type UiSettings } from "$lib/settings/Settings.svelte";
  import { session } from "$lib/state/session.svelte";
  import { previewFixture } from "./ipc";
  let open = $state(true);
  let paired = $state(false);
  let section = $state<Section>("accounts");
  let settings = $state<UiSettings>(structuredClone($state.snapshot(session.settings)));
  let focused = $state("none");
  let calls = $state("");
  const noop = () => {};
  onMount(() => {
    const timer = setInterval(() => (calls = previewFixture.calls.join(", ")), 100);
    return () => clearInterval(timer);
  });
</script>

<svelte:window onfocusin={(event) => {
  if (event.target instanceof HTMLElement) focused = event.target.dataset.settingSearchId ?? event.target.getAttribute("aria-label") ?? event.target.tagName;
}} />

<header>
  <h1>Settings search fixture</h1>
  <button onclick={() => { section = "accounts"; open = true; }}>Open settings</button>
  <label><input type="checkbox" bind:checked={paired} /> Paired sections</label>
</header>
<aside>
  <output aria-label="Selected section">{section}</output>
  <output aria-label="Focused setting">{focused}</output>
  <output aria-label="Native calls">{calls || "none"}</output>
</aside>

{#if open}
  <Settings {settings} bind:section accounts={paired ? [{ id: "settings-fixture", label: "Synthetic account", jid: "123@s.whatsapp.net", once_paired: false }] : []}
    active={paired ? "settings-fixture" : null} me={paired ? "123@s.whatsapp.net" : null} meAvatar={null} accountAvatars={{}}
    onclose={() => (open = false)} onsave={async (next) => { settings = next; }}
    onflush={noop} onclearhistory={noop} onrename={noop} onremove={noop} onadd={noop} onswitch={noop}
    onprivacy={noop} onpicture={noop} onblockedload={async () => []} onunblockcontact={async () => {}} />
{/if}

<style>
  :global(body) { margin: 0; color: var(--text); background: var(--bg); font: 16px system-ui; }
  header { padding: 16px; display: flex; align-items: center; gap: 16px; }
  h1 { margin: 0; font-size: 18px; }
  aside { position: fixed; bottom: 0; inset-inline: 0; z-index: 1000; padding: 8px; background: var(--surface); display: flex; gap: 16px; pointer-events: none; font-size: 12px; }
</style>
