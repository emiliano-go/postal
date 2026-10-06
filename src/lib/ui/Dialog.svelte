<!-- The single native dialog wrapper used by every modal dialog. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import { onDestroy } from "svelte";

  let {
    label = "",
    labelledby = "",
    describedby = "",
    id = "",
    size = "md",
    style = "",
    open = $bindable(false),
    lightDismiss = true,
    onclose,
    onkeydown,
    children,
    dialog = $bindable<HTMLDialogElement | undefined>(undefined),
  }: {
    label?: string;
    labelledby?: string;
    describedby?: string;
    id?: string;
    size?: "sm" | "md" | "lg";
    style?: string;
    open?: boolean;
    lightDismiss?: boolean;
    onclose: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    children: Snippet;
    dialog?: HTMLDialogElement | undefined;
  } = $props();

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });
  onDestroy(() => { dialog?.close(); });
</script>

<dialog
  bind:this={dialog}
  id={id || undefined}
  data-size={size}
  {style}
  aria-label={label || undefined}
  aria-labelledby={labelledby || undefined}
  aria-describedby={describedby || undefined}
  oncancel={(event) => { event.preventDefault(); onclose(); }}
  onclose={() => { if (open) onclose(); }}
  onclick={(event) => { if (lightDismiss && event.target === dialog) onclose(); }}
  {onkeydown}>
  {@render children()}
</dialog>

<style>
  dialog {
    width: var(--dialog-width, min(560px, calc(100vw - 32px)));
    max-height: calc(100vh - 64px);
    overflow: auto;
    box-sizing: border-box;
    padding: 20px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }
  dialog[data-size="sm"] { --dialog-width: min(400px, calc(100vw - 32px)); }
  dialog[data-size="md"] { --dialog-width: min(560px, calc(100vw - 32px)); }
  dialog[data-size="lg"] { --dialog-width: min(760px, calc(100vw - 32px)); }
  dialog::backdrop { background: var(--scrim); }
  @media (max-width: 480px) { dialog { padding: 16px; } }
</style>
