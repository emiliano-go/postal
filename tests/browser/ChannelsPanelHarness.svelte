<script lang="ts">
  import { onMount, tick } from "svelte";
  import ChannelsPanel from "$lib/chat/ChannelsPanel.svelte";
  import type { ChannelSummary, ChannelView } from "$lib/utils/wire";
  import { channelsFixture } from "./ipc";

  const first: ChannelSummary = { jid: "first@newsletter", name: "First Channel", description: "Cached channel", picture_url: null,
    subscriber_count: 90, muted: false, followed: true, favorite: false };
  const found: ChannelSummary = { jid: "found@newsletter", name: "Found Channel", description: "Lookup result", picture_url: null,
    subscriber_count: 120, muted: false, followed: false, favorite: false };
  let connected = $state(true), opened = $state(""), complete = $state(false), failure = $state("");
  const settle = async () => { await tick(); await new Promise((resolve) => setTimeout(resolve, 40)); };
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };

  onMount(() => {
    channelsFixture.view = { channels: [first], synced_at: 100 } as ChannelView;
    channelsFixture.metadata = { [found.jid]: found };
    channelsFixture.pages = {};
    channelsFixture.calls = []; channelsFixture.failure = ""; channelsFixture.defer = ""; channelsFixture.pending = [];
    void (async () => {
      try {
        await settle();
        document.querySelectorAll<HTMLButtonElement>(".tabs button")[2]?.click();
        await tick();
        const input = document.querySelector<HTMLInputElement>("#channel-jid");
        if (!input) throw new Error("Discover JID field missing");
        input.value = found.jid;
        input.dispatchEvent(new Event("input", { bubbles: true }));
        await tick();
        document.querySelector<HTMLButtonElement>(".lookup button")?.click();
        await settle();
        check(document.querySelector(".preview")?.textContent?.includes(found.name), "Channel metadata lookup did not render");
        document.querySelector<HTMLButtonElement>(".preview .btn-primary")?.click();
        await settle();
        check(channelsFixture.calls.some((call) => call.command === "follow_channel"), "Follow did not call backend");
        document.querySelector<HTMLButtonElement>(".preview .open-channel")?.click();
        await settle();
        check(opened === found.jid, "Opening a followed channel did not reach the app");
        check(channelsFixture.calls.some((call) => call.command === "channel_messages" && call.args?.before === null), "Opening a channel did not prime its latest page");
        document.querySelectorAll<HTMLButtonElement>(".tabs button")[0]?.click();
        await settle();
        connected = false;
        await tick();
        const favorite = document.querySelector<HTMLButtonElement>(".row .btn-icon[aria-label]");
        if (!favorite || favorite.disabled) throw new Error("Offline device-local favorite was unavailable");
        favorite.click();
        await settle();
        check(channelsFixture.calls.some((call) => call.command === "set_channel_favorite"), "Device-local favorite did not save");
        const unfollow = document.querySelector<HTMLButtonElement>(".list .unfollow-channel");
        check(unfollow?.disabled, "Offline channel mutation remained enabled");
      } catch (error) { failure = String(error); }
      finally { complete = true; }
    })();
  });
</script>

<ChannelsPanel account="channels-fixture" generation={1} {connected} onOpen={(jid) => (opened = jid)} />
<output id="channels-result" data-complete={complete} data-pass={complete && !failure}>{failure || `${opened || "Channels ready"}; ${channelsFixture.calls.length} channel calls`}</output>

<style>
  :global(:root) { --bg: #10191c; --surface: #182528; --line: #344246; --text: #e9f0ee; --muted: #aab7b4; --danger: #f66; --accent: #8fcfb4; --accent-ink: #13231e; --accent-hover: #a6dec7; --radius-sm: 8px; }
  :global(body) { margin: 0; color: var(--text); background: var(--bg); font: 16px system-ui; }
  output { display: block; padding: 12px; }
</style>
