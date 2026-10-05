<script lang="ts">
  import { onMount, tick } from "svelte";
  import ChatSidebar from "$lib/chat/ChatSidebar.svelte";
  import type { ChatSummary } from "$lib/utils/models";

  const all = Array.from({ length: 5_000 }, (_, index): ChatSummary => ({
    chat: `${String(index).padStart(5, "0")}@s.whatsapp.net`, display_name: `Fixture chat ${index}`,
    last_message_at: 5_000 - index, last_text: `Preview ${index}`, last_from_me: false,
    last_sender_name: null, last_sender: "", last_media_kind: null, message_count: 1,
    unread_count: 0, mention_count: 0, pinned: false, archived: false,
    muted_until: 0, mute_at_all: false, marked_unread: false,
  }));
  let rows = $state.raw(all.slice(0, 64));
  let loads = $state(0);
  let complete = $state(false);
  let pass = $state(false);
  let result = $state("running");
  const noop = () => {};
  const props = $derived({
    searchQuery: "", searchResults: [], visibleChats: rows, hasMore: rows.length < all.length,
    loadingMore: false, onloadmore: () => { loads++; rows = all.slice(0, Math.min(all.length, rows.length + 64)); },
    selectedChat: null, chatFilter: "all" as const, favoriteChats: [], onfilter: noop,
    unreadChats: 0, unreadPings: 0, avatars: {}, chatLabelOf: (chat: ChatSummary) => chat.display_name ?? chat.chat,
    formatTime: () => "12:00", typingLabelOf: () => null, previewAuthorOf: () => null,
    previewTextOf: (chat: ChatSummary) => chat.last_text, mediaIconOf: () => null,
    groupKinds: {}, accounts: [], activeAccount: "fixture", activeLabel: "Fixture",
    accountAvatars: {}, me: null, meVersion: 0, visibility: "online", accountMenu: false,
    onmenutoggle: noop, onswitchaccount: noop, onaddaccount: noop, onsettings: noop,
    onpings: noop, onstarred: noop, onsearch: noop, onopenresult: noop, onopenchat: noop,
    ontogglepin: noop, onclearchat: noop, ondeletechat: noop, onchataction: noop,
    onmarkread: noop, onmarkallread: noop, onnewgroup: noop, onblockcontact: async () => {},
    archivedChats: 0, onresize: noop, freezeOnHover: false, chatPreview: false,
    globalAutoDownload: { image: false, video: false, audio: false, document: false, sticker: false, gif: false },
  });

  onMount(() => {
    void (async () => {
      const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
      const mounted = () => document.querySelectorAll('.chat-viewport [role="listitem"]').length;
      try {
        await tick();
        const scroller = document.querySelector<HTMLElement>('.chat-viewport [role="list"]');
        if (!scroller) throw new Error("missing virtual scroller");
        for (let wait = 0; wait < 30 && mounted() === 0; wait++) await sleep(30);
        const initial = mounted();
        if (!initial || initial > 20 || rows.length !== 64) throw new Error(`initial ${initial}/${rows.length}`);
        for (let attempt = 0; attempt < 4; attempt++) {
          const before: number = rows.length;
          scroller.scrollTop = scroller.scrollHeight;
          scroller.dispatchEvent(new Event("scroll"));
          for (let wait = 0; wait < 30 && rows.length === before; wait++) await sleep(30);
          if (rows.length === before) throw new Error(`page ${attempt} did not load`);
          await tick();
          if (mounted() > 20) throw new Error(`page ${attempt} mounted ${mounted()}`);
        }
        const paged = rows.length;
        rows = all;
        await tick();
        await sleep(60);
        const final = mounted();
        if (final > 20 || final < 1) throw new Error(`5k mounted ${final}`);
        pass = true;
        result = `PASS: initial ${initial}, paged ${paged} through ${loads} scroll loads, 5k mounted ${final}`;
      } catch (error) { result = `FAIL: ${String(error)}`; }
      complete = true;
    })();
  });
</script>

<main><ChatSidebar {...props} /></main>
<output id="chat-sidebar-paging-result" data-complete={complete} data-pass={pass}>{result}</output>

<style>
  :global(body) { margin: 0; font: 14px sans-serif; }
  main { width: 360px; height: 720px; display: flex; }
  main :global(.chats) { width: 100%; }
  output { display: block; padding: 8px; }
</style>
