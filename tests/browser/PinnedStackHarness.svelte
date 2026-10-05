<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import ChatHeader from "$lib/chat/ChatHeader.svelte";
  import { messages as store } from "$lib/state/messages.svelte";
  import { session } from "$lib/state/session.svelte";
  import { floatContent } from "$lib/utils/float-chat";
  import { pinnedMessageIds, stepPinnedMessageId } from "$lib/utils/message-pins";
  import { t } from "$lib/i18n/localizer";
  import type { Marks, StoredMessage } from "$lib/utils/models";
  import { fixture } from "$lib/utils/wire.fixture";
  import { pinFixture } from "./ipc";

  const chat = "pins-fixture@s.whatsapp.net";
  const pinIds = ["pin-80", "pin-40"];
  const allRows = Array.from({ length: 100 }, (_, index) => ({
    ...fixture.message,
    chat,
    id: `pin-${index}`,
    sender: "fixture@s.whatsapp.net",
    sender_name: "Fixture sender",
    from_me: false,
    timestamp: 1_700_000_000 + index,
    sort_order: index,
    text: `Pin target ${index}`,
    media_kind: null,
    system_kind: null,
    read: true,
  }) as StoredMessage);
  const marks = {
    reactions: [], starred: [], pinned: pinIds[0], pinned_messages: pinIds,
    polls: [], events: [], view_once: [], forwarded: [], edited: [],
  } as unknown as Marks;

  let activeId = $state<string | null>(null);
  let preview = $state<StoredMessage | null>(null);
  let pane: HTMLDivElement | undefined = $state();
  let loaded = $state(false), failed = $state("");
  let jumpedId = $state("");
  let request = 0;
  const noop = () => {};
  const pins = $derived(pinnedMessageIds(store.marks));
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };

  $effect(() => {
    const id = activeId, generation = store.accountGeneration, chatId = chat;
    const currentRequest = ++request;
    const present = untrack(() => store.messages.find((message) => message.id === id) ?? null);
    if (present) { preview = present; return; }
    preview = null;
    if (!id) return;
    void store.loadPinnedPreview(chatId, id).then((message) => {
      if (currentRequest === request && generation === store.accountGeneration && activeId === id) preview = message;
    }).catch(() => {});
  });

  const pinnedView = $derived.by(() => {
    const ids = pins, id = activeId;
    if (!id || !ids.includes(id)) return null;
    const message = store.messages.find((row) => row.id === id) ?? (preview?.id === id ? preview : null);
    const content = message ? floatContent(message) : null;
    return {
      id,
      author: message?.sender_name ?? "",
      body: content ? [content.text, content.media].filter(Boolean).join(" · ") || t("chat.pinned_message") : t("chat.pinned_message"),
      position: ids.indexOf(id) + 1,
      count: ids.length,
    };
  });

  function step(direction: -1 | 1) {
    activeId = stepPinnedMessageId(pins, activeId, direction);
  }

  async function jump(id: string) {
    const before = store.messages.map((message) => message.id);
    const scrollTop = pane?.scrollTop ?? 0;
    const row = store.messages.find((message) => message.id === id)
      ?? (preview?.id === id ? preview : await store.loadPinnedPreview(chat, id));
    if (!row) {
      const result = await store.showStoredMessage(chat, id);
      if (!result && JSON.stringify(store.messages.map((message) => message.id)) === JSON.stringify(before)
        && (pane?.scrollTop ?? 0) === scrollTop) jumpedId = "failed-with-window-preserved";
      else failed = "Missing pin changed loaded message window or scroll position";
      return;
    }
    if (!store.messages.some((message) => message.id === id)) {
      const result = await store.showStoredMessage(chat, id);
      if (!result) {
        if (JSON.stringify(store.messages.map((message) => message.id)) === JSON.stringify(before) && (pane?.scrollTop ?? 0) === scrollTop) jumpedId = "failed-with-window-preserved";
        else failed = "Failed pin load changed the message window or scroll position";
        return;
      }
    }
    await tick();
    jumpedId = id;
  }

  onMount(() => {
    pinFixture.rows = allRows;
    pinFixture.marks = marks;
    pinFixture.calls.splice(0);
    pinFixture.failNextAnchor = "";
    session.activeAccount = "pins-fixture";
    store.prepareChat(chat, 50);
    store.acceptMessages(allRows.slice(-12).reverse());
    store.marks = marks;
    activeId = pinIds[0];

    void (async () => {
      try {
        await tick(); await new Promise((resolve) => setTimeout(resolve, 80));
        check(store.messages.length === 12 && !store.messages.some((message) => pinIds.slice(0, 2).includes(message.id)), "Fixture pins must start outside loaded window");
        check(pinnedView?.id === "pin-80" && pinnedView.body.includes("Pin target 80"), "First pin preview missing");
        document.querySelectorAll<HTMLButtonElement>(".pinned-bar button[aria-label]")[1]?.click();
        await tick(); await new Promise((resolve) => setTimeout(resolve, 80));
        check(pinnedView?.id === "pin-40" && pinnedView.position === 2, "Pin stack did not advance in backend order");
        document.querySelectorAll<HTMLButtonElement>(".pinned-bar button[aria-label]")[0]?.click();
        await tick();
        check(pinnedView?.id === "pin-80" && pinnedView.position === 1, "Previous pin did not follow backend order");
        document.querySelectorAll<HTMLButtonElement>(".pinned-bar button[aria-label]")[1]?.click();
        await tick(); await new Promise((resolve) => setTimeout(resolve, 80));
        const currentPane = pane;
        if (!currentPane) throw new Error("Loaded message pane missing");
        currentPane.scrollTop = 60;
        pinFixture.failNextAnchor = "pin-40";
        await jump("pin-40");
        check(jumpedId === "failed-with-window-preserved" && store.messages.length === 12, "Missing pin changed the viewport window");
        activeId = stepPinnedMessageId(pins, activeId, -1);
        await tick(); await new Promise((resolve) => setTimeout(resolve, 80));
        await jump("pin-80");
        check(jumpedId === "pin-80" && store.messages.length === 1 && store.messages[0].id === "pin-80", "Off-window pin did not load directly");
        check(pinFixture.calls.some((call) => call.limit === 1)
          && pinFixture.calls.every((call) => !!call.id && call.limit > 0 && call.limit <= store.messageLimit), "Pin preview fetched unrelated history");
      } catch (error) { failed = String(error); }
      finally { loaded = true; }
    })();
  });
</script>

<ChatHeader selectedChat={chat} isGroup={false} title="Pin fixture" avatar={null} typingNow={null} subtitle={null}
  groupContext={null} presenceText={null} mentionTotal={0} mentionCursor={0} pinned={pinnedView} ongroupinfo={noop} onsearch={noop}
  ongallery={noop} onpings={noop} onsettings={noop} onjumpmention={noop} onpinnedjump={(id) => void jump(id)}
  onpinnedprevious={() => step(-1)} onpinnednext={() => step(1)} onclearchat={noop} ondeletechat={noop} />
<div class="loaded" bind:this={pane} aria-label="Loaded message window">
  {#each store.ordered as message (message.id)}<p data-id={message.id}>{message.text}</p>{/each}
</div>
<output id="pinned-stack-result" data-complete={loaded} data-pass={loaded && !failed}>
  {failed || `${jumpedId || "Pin stack ready"}; ${pinFixture.calls.length} anchored page requests`}
</output>

<style>
  :global(body) { margin: 0; color: var(--text); background: var(--bg); font: 16px system-ui; }
  .loaded { height: 220px; overflow: auto; }
  .loaded p { min-height: 44px; padding: 12px; margin: 0; border-bottom: 1px solid var(--line); }
  output { display: block; padding: 12px; }
</style>
