<!-- The small confirm sheet shared by delete/report style actions. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Dialog from "$lib/ui/Dialog.svelte";

  let {
    label,
    title,
    hint,
    actions,
    onclose,
  }: {
    label: string;
    title: string;
    hint: string;
    actions: Snippet;
    onclose: () => void;
  } = $props();

  function escape(event: KeyboardEvent) {
    if (event.key === "Escape") event.stopPropagation();
  }
</script>

<Dialog
  open={true}
  {label}
  size="sm"
  style="width:min(400px,90vw);max-height:86vh;padding:0;border:0;background:transparent;box-shadow:none;overflow:visible;"
  {onclose}
  onkeydown={escape}>
  <div class="sheet confirm">
    <h2>{title}</h2>
    <p class="hint">{hint}</p>
    <div class="confirm-actions">{@render actions()}</div>
  </div>
</Dialog>

<style>
  .sheet {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: 20px 22px;
    width: min(460px, 90vw);
    max-height: 86vh;
    overflow-y: auto;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .sheet.confirm {
    width: min(400px, 90vw);
  }
  .sheet.confirm h2 {
    margin: 0;
    font-size: 1.0625rem;
    font-weight: 600;
  }
  .hint {
    margin: 0;
    color: var(--faint);
    font-size: 0.75rem;
    max-width: 44ch;
    text-wrap: balance;
  }
  .confirm-actions {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
  .confirm-actions :global(button) {
    padding: 8px 14px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--accent);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .confirm-actions :global(button:hover) {
    background: var(--raised);
  }
  .confirm-actions :global(button.danger) {
    color: var(--danger);
  }
</style>
