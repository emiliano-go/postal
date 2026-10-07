<script lang="ts">
  import { onMount, tick } from "svelte";
  import UnifiedInbox from "$lib/chat/UnifiedInbox.svelte";
  import type { ChatSummary, MessageLabelAssociation, StoredMessage } from "$lib/utils/wire";
  import type { InboxLabel } from "$lib/utils/inbox";
  import { labelledMessagesFixture } from "./ipc";

  let account = $state("inbox-a"), requestKey = $state("1"), connected = $state(true);
  let chats = $state<ChatSummary[]>([
    { chat: "unread@s", display_name: "Unread chat", unread_count: 2, mention_count: 0, muted_until: 0, archived: false, marked_unread: false } as unknown as ChatSummary,
    { chat: "quiet@s", display_name: "Quiet chat", unread_count: 0, mention_count: 0, muted_until: 0, archived: false, marked_unread: false } as unknown as ChatSummary,
    { chat: "archived@s", display_name: "Archived chat", unread_count: 0, mention_count: 0, muted_until: 0, archived: true, marked_unread: false } as unknown as ChatSummary,
  ]);
  let labels = $state<InboxLabel[]>([{ id: "important", name: "Important" }, { id: "personal", name: "Personal" }]);
  let messageLabels = $state<MessageLabelAssociation[]>([
    { chat: "unread@s", message_id: "m1", label_id: "important" },
    { chat: "quiet@s", message_id: "m2", label_id: "important" },
    { chat: "archived@s", message_id: "m3", label_id: "personal" },
  ]);
  let opened = "", checks = $state<string[]>([]), failure = $state(""), complete = $state(false);
  const settle = async () => { await tick(); await new Promise((resolve) => setTimeout(resolve, 30)); };
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  async function until(condition: () => unknown) {
    const end = performance.now() + 4000;
    while (!condition()) { if (performance.now() > end) throw new Error("labelled Inbox fixture timed out"); await settle(); }
    await tick();
  }
  const chat = (jid: string) => chats.find((item) => item.chat === jid)!;
  const row = (id: string, jid: string, text: string, timestamp: number): StoredMessage => ({
    id, chat: jid, text, timestamp, sort_order: timestamp, from_me: false, sender: "sender@s", sender_name: "Sender",
    media_kind: null, spoiler: false, deleted: false, revoked: false,
  } as unknown as StoredMessage);
  const labelFor = (item: ChatSummary) => item.display_name ?? item.chat;
  const baseRows = [row("m1", "unread@s", "Follow up customer account", 300), row("m2", "quiet@s", "Customer update", 200), row("m3", "archived@s", "Internal note", 100)];
  function resetRows(rows = baseRows) {
    labelledMessagesFixture.rows = rows;
    labelledMessagesFixture.labelsByMessage = Object.fromEntries(rows.map((message) =>
      [JSON.stringify([message.chat, message.id]), [message.id === "m3" ? "personal" : "important"]]));
    messageLabels = rows.map((message) => ({ chat: message.chat, message_id: message.id,
      label_id: message.id === "m3" ? "personal" : "important" }));
  }
  const write = async (input: HTMLInputElement, value: string) => {
    input.value = value; input.dispatchEvent(new InputEvent("input", { bubbles: true })); await tick();
  };

  labelledMessagesFixture.calls = []; labelledMessagesFixture.failure = ""; labelledMessagesFixture.deferNext = false; labelledMessagesFixture.pending = [];
  resetRows();

  onMount(() => { void (async () => {
    try {
      await settle();
      check(document.querySelectorAll(".message-entry").length === 0, "ordinary chat Inbox changed before message mode");
      const labelSelect = document.querySelectorAll<HTMLSelectElement>(".choice select")[0];
      labelSelect.value = "important"; labelSelect.dispatchEvent(new Event("change", { bubbles: true }));
      await until(() => document.querySelectorAll(".message-entry").length === 2);
      const exact = document.querySelector<HTMLButtonElement>('.message-entry[aria-label]') ?? document.querySelector<HTMLButtonElement>(".message-entry")!;
      check(document.querySelector(".message-label")?.textContent === "Important", "message label chip missing");
      check(labelledMessagesFixture.calls.at(-1)?.labelIds instanceof Array
        && JSON.stringify(labelledMessagesFixture.calls.at(-1)?.labelIds) === '["important"]', "selected label was not queried");
      exact.click(); await tick();
      check(opened === "unread@s:m1" || opened === "quiet@s:m2", "message row did not open exact message");
      checks.push("selected label renders matching messages with chips and exact-message open");

      messageLabels = messageLabels.filter((item) => item.message_id !== "m1");
      delete labelledMessagesFixture.labelsByMessage[JSON.stringify(["unread@s", "m1"])];
      await until(() => document.querySelectorAll(".message-entry").length === 1);
      check(!document.querySelector(".inbox")?.textContent?.includes("Follow up customer account"), "association removal left a stale row");
      messageLabels = [...messageLabels, { chat: "quiet@s", message_id: "m4", label_id: "important" }];
      resetRows([...baseRows, row("m4", "quiet@s", "Newly labelled message", 400)]);
      delete labelledMessagesFixture.labelsByMessage[JSON.stringify(["unread@s", "m1"])];
      await until(() => document.querySelectorAll(".message-entry").length === 2);
      check(document.querySelector(".inbox")?.textContent?.includes("Newly labelled message"), "association addition did not refresh rows");
      checks.push("label association changes refresh and remove or add matching rows");

      const query = document.querySelector<HTMLInputElement>(".filters .search")!;
      await write(query, "customer account");
      await until(() => (labelledMessagesFixture.calls.at(-1)?.query === "customer account"));
      check(labelledMessagesFixture.calls.at(-1)?.chatIds === null, "unfiltered triage sent a chat whitelist");
      checks.push("message search reaches the bounded backend query");

      const unreadFilter = document.querySelectorAll<HTMLInputElement>("fieldset input[type=checkbox]")[0];
      unreadFilter.click(); await tick();
      await until(() => labelledMessagesFixture.calls.at(-1)?.chatIds !== null);
      check(JSON.stringify(labelledMessagesFixture.calls.at(-1)?.chatIds) === '["unread@s"]', "unread triage did not prefilter chats before the row cap");
      checks.push("unread triage is applied by the bounded query");

      chats = chats.map((item) => item.chat === "unread@s" ? { ...item, unread_count: 0 }
        : item.chat === "quiet@s" ? { ...item, unread_count: 1 } : item);
      await until(() => JSON.stringify(labelledMessagesFixture.calls.at(-1)?.chatIds) === '["quiet@s"]');
      checks.push("live chat metadata refresh changes the pre-limit triage whitelist");

      unreadFilter.click(); await tick();
      await until(() => labelledMessagesFixture.calls.at(-1)?.chatIds === null);
      labelledMessagesFixture.deferNext = true;
      resetRows([row("old-query", "unread@s", "old query result", 400)]);
      await write(query, "old query");
      await until(() => labelledMessagesFixture.pending.length === 1);
      resetRows([row("new-query", "quiet@s", "new query result", 500)]);
      await write(query, "new query");
      await until(() => document.querySelector(".preview")?.textContent?.includes("new query result"));
      labelledMessagesFixture.pending.shift()?.(); await settle();
      check(!document.querySelector(".inbox")?.textContent?.includes("old query result"), "stale query result replaced current message rows");
      checks.push("stale query completion is fenced");

      labelledMessagesFixture.deferNext = true;
      resetRows([row("old-account", "unread@s", "old account result", 600)]);
      await write(query, "account");
      await until(() => labelledMessagesFixture.pending.length === 1);
      account = "inbox-b"; requestKey = "2";
      resetRows([row("new-account", "quiet@s", "new account result for account query", 700)]);
      await settle();
      labelSelect.value = "important"; labelSelect.dispatchEvent(new Event("change", { bubbles: true }));
      await until(() => document.querySelector(".preview")?.textContent?.includes("new account result"));
      labelledMessagesFixture.pending.shift()?.(); await settle();
      check(!document.querySelector(".inbox")?.textContent?.includes("old account result"), "stale account result crossed scope");
      checks.push("account/request switch fences stale responses");

      resetRows([]);
      await until(() => document.querySelector(".message-entry") === null && document.querySelector(".status")?.textContent?.includes("No labelled"));
      checks.push("empty label results are explicit");

      labelledMessagesFixture.failure = "Synthetic labelled query failure";
      const retry = document.querySelector<HTMLInputElement>(".filters .search")!;
      await write(retry, "retry-trigger");
      await until(() => document.querySelector('[role="alert"]')?.textContent?.includes("Operation failed")
        && [...document.querySelectorAll<HTMLButtonElement>(".error button")].some((button) => button.textContent?.includes("Retry")));
      checks.push("query failure remains visible");

      labelledMessagesFixture.failure = "";
      resetRows([row("retry-success", "unread@s", "retry-trigger result", 900)]);
      const retryButton = [...document.querySelectorAll<HTMLButtonElement>(".error button")].find((item) => item.textContent?.includes("Retry"));
      retryButton?.click();
      await until(() => document.querySelector(".message-entry .preview")?.textContent?.includes("retry-trigger result"));
      checks.push("retry reruns the failed labelled query");

      resetRows(Array.from({ length: 500 }, (_, index) => row(`cap-${index}`, "unread@s", `Capped message ${index}`, 1000 - index)));
      await write(retry, "Capped message");
      await until(() => document.querySelector(".status")?.textContent?.includes("latest 500"));
      check(document.querySelectorAll(".message-entry").length === 500, "500-row cap was not respected");
      checks.push("500 matching rows show an explicit cap without a false-complete claim");
      complete = true;
    } catch (error) { failure = String(error); }
  })(); });
</script>

<UnifiedInbox {account} {requestKey} {connected} {chats} labels={labels} labelsByChat={{}} {messageLabels}
  labelsWritable={true} labelsComplete={true} chatLabelOf={labelFor} formatTime={(timestamp) => String(timestamp)}
  onopen={(jid) => { opened = jid; }} onopenmessage={(jid, id) => { opened = `${jid}:${id}`; }}
  onaction={async () => {}} initialFilters={{ unread: false, mentions: false, labelled: false, muted: false, archived: false, label: "", query: "" }} />
<output id="labelled-inbox-result" data-complete={complete} data-pass={complete && !failure}>{failure || `${checks.length} passed`}</output>
<ul>{#each checks as item}<li>{item}</li>{/each}</ul>

<style>
  :global(:root) { --bg: #10191c; --surface: #182528; --raised: #223236; --line: #344246; --line-strong: #526267; --text: #e9f0ee; --muted: #aab7b4; --danger: #f66; --accent: #8fcfb4; --accent-soft: #203b31; --accent-text: #8fcfb4; --radius-sm: 8px; }
  :global(body) { margin: 0; color: var(--text); background: var(--bg); font: 16px system-ui; }
  output, ul { display: block; padding: 12px; }
</style>
