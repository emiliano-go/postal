<script lang="ts">
  import MessageFinder from "$lib/messages/MessageFinder.svelte";

  let opened = $state(true);
  let outcome = $state("");

  function statusFor(date: string): "found" | "offline" | "missing" | "limit" | "unavailable" {
    return ({ "01": "offline", "02": "missing", "03": "limit", "04": "unavailable" } as Record<string, "offline" | "missing" | "limit" | "unavailable">)[date.slice(-2)] ?? "found";
  }

  async function jump(date: string, progress: (pages: number) => void, signal: AbortSignal) {
    const result = statusFor(date);
    return await new Promise<"found" | "offline" | "unavailable" | "missing" | "cancelled" | "limit">((resolve) => {
      const loading = setTimeout(() => progress(1), 450);
      const finish = setTimeout(() => {
        signal.removeEventListener("abort", cancel);
        outcome = `${date}: ${result}`;
        if (result === "found") opened = false;
        resolve(result);
      }, 2_000);
      function cancel() {
        clearTimeout(loading);
        clearTimeout(finish);
        outcome = "cancelled before any window change";
        resolve("cancelled");
      }
      signal.addEventListener("abort", cancel, { once: true });
    });
  }
</script>

<button onclick={() => { opened = true; }}>Open date finder</button>
<output aria-label="Synthetic date outcome">{outcome}</output>
{#if opened}
  <MessageFinder title="Synthetic chat search" subtitle="Date jump fixture" placeholder="Search this chat"
    items={[]} empty="No synthetic search results" initialQuery="" ondatejump={jump}
    onopen={() => {}} onclose={() => { opened = false; }} />
{/if}

<style>
  :global(:root) { --bg: #132029; --surface: #1b2c36; --raised: #29414d; --text: #eee; --muted: #b7c8d3; --accent: #2cd4a0; --line-strong: #547080; --scrim: #0008; --radius-lg: 12px; --shadow: 0 8px 40px #0008; --faint: #9eacb6; }
  :global(body) { margin: 0; font: 14px system-ui; background: var(--bg); color: var(--text); }
  button, output { margin: 12px; }
</style>
