<script lang="ts">
  import { tick } from "svelte";
  import Panel from "$lib/ui/Panel.svelte";
  import AddMembers from "$lib/chat/AddMembers.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import type { GroupHistoryResult, GroupMemberAddResult } from "$lib/utils/models";

  const nav = [{ id: "general", label: "General", group: "Group" }];
  let section = $state("general");
  let adding = $state(false), confirming = $state(false), check = $state("waiting");

  async function inspectOverlay() {
    await tick();
    requestAnimationFrame(() => {
      const dialog = document.querySelector<HTMLDialogElement>("dialog[open]");
      const rect = dialog?.getBoundingClientRect();
      const top = rect && document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
      check = dialog?.matches(":modal") && !!top && dialog.contains(top) ? "PASS: dialog is topmost and interactive" : "FAIL: dialog is behind the panel";
    });
  }

  const add = async (): Promise<GroupMemberAddResult> => ({ participants: [], history: { state: "not_requested", message: "No history requested.", retry_id: null } });
  const retry = async (): Promise<GroupHistoryResult> => ({ state: "not_requested", message: "No history requested.", retry_id: null });

  $effect(() => { if (adding || confirming) void inspectOverlay(); });
</script>

<Panel label="Synthetic group settings" {nav} bind:section onclose={() => { adding = false; confirming = false; }}>
  {#snippet header()}<strong>Group settings</strong>{/snippet}
  <h1>Synthetic group</h1>
  <button onclick={() => { adding = true; }}>Add members</button>
  <button onclick={() => { confirming = true; }}>Remove members</button>
</Panel>

{#if adding}
  <AddMembers chat="synthetic@g.us" title="Synthetic group" members={[]} avatars={{}} me={null}
    onavatar={() => {}} onadd={add} onretryhistory={retry} onclose={() => { adding = false; }} />
{/if}

{#if confirming}
  <ConfirmDialog label="Remove members" title="Remove selected members?" hint="Synthetic confirmation only."
    onclose={() => { confirming = false; }}>
    {#snippet actions()}<button onclick={() => { confirming = false; }}>Cancel</button>{/snippet}
  </ConfirmDialog>
{/if}

<output aria-label="Overlay check">{check}</output>

<style>
  :global(:root) { --bg: #132029; --surface: #1b2c36; --raised: #29414d; --text: #eee; --muted: #b7c8d3; --accent: #2cd4a0; --accent-text: #0c241b; --line-strong: #547080; --scrim: #0008; --radius-lg: 12px; --shadow: 0 8px 40px #0008; --faint: #9eacb6; }
  :global(body) { font: 14px system-ui; background: var(--bg); color: var(--text); }
  h1, button, output { margin: 12px; }
</style>
