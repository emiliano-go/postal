<script lang="ts">
  import { onMount, tick } from "svelte";
  import MessageMenu from "$lib/messages/MessageMenu.svelte";
  import { ui } from "$lib/state/ui.svelte";
  import { t } from "$lib/i18n/localizer";
  import type { StoredMessage } from "$lib/utils/wire";

  const messages = { reactionsFor: new Map() };
  const chats = { selectedChat: "menu@test" };
  const messageList = { focusRail() {} };
  const quickReactions: string[] = [];
  const menuItems = (message: StoredMessage) => [{ label: `Copy ${message.id}`, action() {} }];
  const broadcastSendReason = () => null;
  const reactMessages = () => {};
  const openEmojiFor = () => {};
  let complete = $state(false), failure = $state("");
  const settle = async () => { await tick(); await new Promise((resolve) => setTimeout(resolve, 300)); };
  const open = (id: string) => { ui.menu = { x: 80, y: 80, message: { id, chat: chats.selectedChat } as StoredMessage }; };
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };

  onMount(() => {
    const onError = (event: ErrorEvent) => { failure = event.message; };
    window.addEventListener("error", onError);
    void (async () => {
      try {
        open("A");
        await settle();
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
        await settle();
        check(ui.menu === null, "Ordinary menu close did not clear state");
        check(!failure, failure);
        open("A");
        await settle();
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
        await tick();
        open("B");
        await settle();
        check(ui.menu?.message.id === "B", "Stale menu close dismissed replacement");
        check(document.querySelector(".menu")?.textContent?.includes("Copy B"), "Replacement menu not visible");
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
        await settle();
        check(ui.menu === null, "Replacement menu did not close");
        check(!failure, failure);
      } catch (error) { failure = String(error); }
      finally { complete = true; }
    })();
    return () => { window.removeEventListener("error", onError); ui.menu = null; };
  });
</script>

<!-- production-menu -->
<output id="menu-lifecycle-result" data-complete={complete} data-pass={complete && !failure}>{failure || (complete ? "Menu close and replacement passed" : "Checking menu lifecycle…")}</output>
