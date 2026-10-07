<script lang="ts">
  import { onMount, tick } from "svelte";
  import CallsPanel from "$lib/chat/CallsPanel.svelte";
  import { callHistory } from "$lib/state/call-history.svelte";
  import { dispatchServiceEvent, type EventHost } from "$lib/state/events";
  import type { CallRecord } from "$lib/utils/wire";
  import { callHistoryFixture } from "./ipc";

  let account = $state("calls-a"), generation = $state(1), connected = $state(true);
  let opened = $state(""), checks = $state<string[]>([]), failure = $state(""), complete = $state(false);
  const host: EventHost = { scrollToBottom() {}, anchor: () => null, reveal: () => true, reconnect: async () => {} };
  const settle = async () => { await tick(); await new Promise((resolve) => setTimeout(resolve, 30)); };
  const check = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  async function until(condition: () => unknown) {
    const end = performance.now() + 4000;
    while (!condition()) { if (performance.now() > end) throw new Error("calls panel fixture timed out"); await settle(); }
    await tick();
  }
  function row(id: string, chat: string, patch: Partial<CallRecord> = {}): CallRecord {
    return { call_id: id, creator_jid: chat, peer_jid: chat, group_jid: null, chat, from_me: false, is_video: true,
      outcome: "missed", outcome_raw: 4, call_type_raw: null, start_time_raw: null, duration_raw: null,
      mutation_at_ms: 1_800_000_000_000, from_full_sync: false, ...patch };
  }
  function refreshEvent() { return dispatchServiceEvent({ kind: "callHistoryChanged" }, host); }
  const nameOf = (jid: string) => jid === "alice@s" ? "Alice" : jid === "old@s" ? "Old contact" : jid;

  callHistoryFixture.calls = []; callHistoryFixture.records = [row("first", "alice@s")];
  callHistoryFixture.failure = ""; callHistoryFixture.deferNext = true; callHistoryFixture.pending = [];

  onMount(() => { void (async () => {
    try {
      await until(() => callHistoryFixture.pending.length === 1 && document.querySelector(".calls .status"));
      check(document.querySelector(".calls .status")?.textContent?.includes("Loading calls"), "initial loading state missing");
      callHistoryFixture.pending.shift()?.();
      await until(() => document.querySelector(".entry")?.textContent?.includes("Alice"));
      const entry = document.querySelector<HTMLButtonElement>(".entry")!;
      check(entry.textContent?.includes("Missed") && entry.textContent.includes("Video call"), "direction/type did not render");
      check(entry.textContent?.includes("Date unavailable") && entry.textContent.includes("Duration unavailable"), "unavailable call clocks were not explicit");
      entry.click(); await tick();
      check(opened === "alice@s", "entry did not open its chat");
      checks.push("missed video row renders honest clock state and opens chat");

      callHistoryFixture.records = [];
      await refreshEvent();
      await until(() => document.querySelector(".empty")?.textContent?.includes("No calls"));
      checks.push("empty state renders after a live call-history event");

      callHistoryFixture.failure = "Synthetic call-history failure";
      await refreshEvent();
      await until(() => document.querySelector('[role="alert"]')?.textContent?.includes("Operation failed"));
      check(document.querySelector('[role="alert"]')?.textContent?.includes("Synthetic call-history failure"), "error diagnostic missing");
      checks.push("load errors remain visible");

      callHistoryFixture.failure = "";
      callHistoryFixture.records = [row("fresh", "fresh@s", { from_me: true, is_video: false, outcome: "connected", outcome_raw: 0 })];
      await refreshEvent();
      await until(() => document.querySelector(".entry")?.textContent?.includes("fresh@s"));
      checks.push("event refresh replaces the visible call rows");

      callHistoryFixture.deferNext = true;
      callHistoryFixture.records = [row("stale-account", "old@s")];
      await refreshEvent();
      await until(() => callHistoryFixture.pending.length === 1);
      account = "calls-b"; generation = 2;
      callHistoryFixture.records = [row("current-account", "new@s")];
      await until(() => document.querySelector(".entry")?.textContent?.includes("new@s"));
      callHistoryFixture.pending.shift()?.(); await settle();
      check(!document.querySelector(".calls")?.textContent?.includes("Old contact")
        && document.querySelector(".entry")?.textContent?.includes("new@s"), "stale account result replaced current rows");
      checks.push("account switch fences stale result and loading completion");

      callHistoryFixture.deferNext = true;
      callHistoryFixture.failure = "Synthetic stale failure";
      await refreshEvent();
      await until(() => callHistoryFixture.pending.length === 1);
      callHistoryFixture.failure = "";
      callHistoryFixture.records = [row("current-generation", "latest@s")];
      generation++;
      await until(() => document.querySelector(".entry")?.textContent?.includes("latest@s"));
      callHistoryFixture.pending.shift()?.(); await settle();
      check(!document.querySelector('[role="alert"]') && document.querySelector(".entry")?.textContent?.includes("latest@s"),
        "stale rejection replaced current error/data state");
      checks.push("generation switch fences stale error completion");
      complete = true;
    } catch (error) { failure = String(error); }
  })(); });
</script>

<CallsPanel {account} {generation} {connected} refreshKey={callHistory.revision} labelFor={nameOf} onopen={(chat) => { opened = chat; }} />
<output id="calls-panel-result" data-complete={complete} data-pass={complete && !failure}>{failure || `${checks.length} passed`}</output>
<ul>{#each checks as item}<li>{item}</li>{/each}</ul>

<style>
  :global(:root) { --bg: #10191c; --surface: #182528; --raised: #223236; --line: #344246; --text: #e9f0ee; --muted: #aab7b4; --danger: #f66; --accent: #8fcfb4; --radius-sm: 8px; }
  :global(body) { margin: 0; color: var(--text); background: var(--bg); font: 16px system-ui; }
  output, ul { display: block; padding: 12px; }
</style>
