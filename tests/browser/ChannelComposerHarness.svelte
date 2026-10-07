<script lang="ts">
  import { onMount, tick } from "svelte";
  import ChannelComposer from "$lib/chat/ChannelComposer.svelte";
  import type { StoredMessage } from "$lib/utils/wire";

  let account = $state("fixture-a"), generation = $state(1), chat = $state("admin@newsletter");
  let connected = $state(true), canPost = $state(true), busyChat = $state<string | null>(null);
  let editing = $state<StoredMessage | null>(null), checks = $state<string[]>([]), failure = $state(""), complete = $state(false);
  let submitted: { kind: string; values: unknown[]; account: string; chat: string }[] = [];
  let deferText = false, releaseText: ((success: boolean) => void) | null = null;
  const busy = $derived(busyChat === chat);
  const settle = async () => { await tick(); await new Promise((resolve) => setTimeout(resolve, 20)); };
  const check = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  async function until(condition: () => unknown) {
    const end = performance.now() + 3000;
    while (!condition()) { if (performance.now() > end) throw new Error("fixture timed out"); await settle(); }
    await tick();
  }
  function button(label: string) {
    return [...document.querySelectorAll<HTMLButtonElement>(".channel-composer button")]
      .find((element) => element.textContent?.trim() === label);
  }
  async function value(element: HTMLInputElement | HTMLTextAreaElement, text: string) {
    element.value = text;
    element.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
    await tick();
  }
  async function ontext(text: string) {
    const scope = { account, chat };
    if (deferText) {
      deferText = false;
      busyChat = chat;
      return await new Promise<boolean>((resolve) => {
        releaseText = (success) => { busyChat = null; submitted.push({ kind: "text", values: [text], ...scope }); resolve(success); };
      });
    }
    submitted.push({ kind: "text", values: [text], ...scope });
    return text !== "fail";
  }
  async function onmedia(file: File, caption: string) {
    submitted.push({ kind: "media", values: [file.name, caption], account, chat });
    return true;
  }
  async function onpoll(question: string, options: string[], multi: boolean) {
    submitted.push({ kind: "poll", values: [question, options, multi], account, chat });
    return true;
  }
  async function onedit(id: string, text: string) {
    submitted.push({ kind: "edit", values: [id, text], account, chat });
    return true;
  }
  const cancelEdit = () => { editing = null; };
  function record(label: string) { checks.push(label); }

  onMount(() => { void (async () => {
    try {
      await settle();
      const text = () => document.querySelector<HTMLTextAreaElement>("textarea.text")!;
      await value(text(), "A channel post"); button("Publish")!.click();
      await until(() => submitted.some((item) => item.kind === "text"));
      check(submitted.at(-1)?.values[0] === "A channel post" && text().value === "", "text post did not submit and clear on acknowledgement");
      record("admin text post submits and clears on success");

      const fileInput = document.querySelector<HTMLInputElement>('.channel-composer input[type="file"]')!;
      const file = new File(["fixture image"], "channel.png", { type: "image/png" });
      Object.defineProperty(fileInput, "files", { configurable: true, value: [file] });
      fileInput.dispatchEvent(new Event("change", { bubbles: true })); await tick();
      const caption = document.querySelector<HTMLTextAreaElement>(".channel-composer .field-label textarea.field")!;
      await value(caption, "Photo caption"); button("Publish")!.click();
      await until(() => submitted.some((item) => item.kind === "media"));
      check(JSON.stringify(submitted.find((item) => item.kind === "media")?.values) === JSON.stringify(["channel.png", "Photo caption"]), "media post payload mismatch");
      record("admin media post submits file and caption");

      button("Create poll")!.click(); await tick();
      const pollQuestion = document.querySelector<HTMLInputElement>(".poll-form .field-label input")!;
      const options = document.querySelectorAll<HTMLInputElement>(".poll-option input");
      await value(pollQuestion, "Channel poll"); await value(options[0], "One"); await value(options[1], "Two");
      document.querySelector<HTMLInputElement>(".poll-actions input[type=checkbox]")!.click(); await tick();
      button("Publish poll")!.click();
      await until(() => submitted.some((item) => item.kind === "poll"));
      check(JSON.stringify(submitted.find((item) => item.kind === "poll")?.values) === JSON.stringify(["Channel poll", ["One", "Two"], true]), "poll payload mismatch");
      record("admin poll submits question, options and multi-vote choice");

      editing = { id: "post-1", chat, text: "Original post", media_kind: null } as StoredMessage; await settle();
      check(text().value === "Original post", "edit did not load original text");
      await value(text(), "Edited post"); button("Save edit")!.click();
      await until(() => submitted.some((item) => item.kind === "edit"));
      check(JSON.stringify(submitted.find((item) => item.kind === "edit")?.values) === JSON.stringify(["post-1", "Edited post"]) && editing === null, "edit payload or completion mismatch");
      record("admin edit loads original text and submits changed text");

      await value(text(), "fail"); button("Publish")!.click(); await until(() => submitted.some((item) => item.values[0] === "fail"));
      check(text().value === "fail", "failed callback cleared the draft");
      record("failed callback preserves text draft");

      canPost = false; await tick();
      check(!document.querySelector(".channel-composer"), "reader still had a channel composer");
      canPost = true; connected = false; await tick();
      check(text().disabled && button("Publish")?.disabled && button("Add media")?.disabled && button("Create poll")?.disabled, "offline controls remained enabled");
      connected = true; await tick(); record("reader role hides authoring controls and offline state disables them");

      for (const transition of ["account", "chat", "generation"] as const) {
        await value(text(), `pending ${transition}`); deferText = true; button("Publish")!.click();
        await until(() => releaseText !== null);
        if (transition === "account") { account = "fixture-b"; busyChat = null; }
        else if (transition === "chat") chat = "other@newsletter";
        else generation++;
        await settle(); await value(text(), `${transition} new draft`);
        const release = releaseText!; releaseText = null; release(true);
        await settle();
        check(text().value === `${transition} new draft`, `${transition} switch completion cleared the new draft`);
        record(`${transition} switch preserves draft while old send resolves`);
      }
      complete = true;
    } catch (error) { failure = String(error); }
  })(); });
</script>

{#if canPost}
  <ChannelComposer {account} {generation} {chat} {connected} {canPost} {busy} {editing}
    ontext={ontext} onmedia={onmedia} onpoll={onpoll} onedit={onedit} oncancelEdit={cancelEdit} />
{:else}
  <div class="read-only" role="status">Channel is read only</div>
{/if}
<output id="channel-composer-result" data-complete={complete} data-pass={complete && !failure}>{failure || `${checks.length} passed`}</output>
<ul>{#each checks as item}<li>{item}</li>{/each}</ul>

<style>
  :global(:root) { --bg: #10191c; --surface: #182528; --line: #344246; --text: #e9f0ee; --muted: #aab7b4; --danger: #f66; --accent: #8fcfb4; --accent-ink: #13231e; --accent-hover: #a6dec7; --radius-sm: 8px; }
  :global(body) { margin: 0; color: var(--text); background: var(--bg); font: 16px system-ui; }
  output, ul { display: block; padding: 12px; }
</style>
