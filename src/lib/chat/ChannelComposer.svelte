<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import type { LocalizedError } from "$lib/i18n/errors";
  import Button from "$lib/ui/Button.svelte";
  import type { StoredMessage } from "$lib/utils/wire";

  let {
    account,
    generation,
    chat,
    connected,
    canPost,
    busy = false,
    error = null,
    editing = null,
    ontext,
    onedit,
    onmedia,
    onpoll,
    oncancelEdit,
  }: {
    account: string;
    generation: number;
    chat: string;
    connected: boolean;
    canPost: boolean;
    busy?: boolean;
    error?: LocalizedError | null;
    editing?: StoredMessage | null;
    ontext: (text: string) => Promise<boolean>;
    onedit: (id: string, text: string) => Promise<boolean>;
    onmedia: (file: File, caption: string) => Promise<boolean>;
    onpoll: (question: string, options: string[], multi: boolean) => Promise<boolean>;
    oncancelEdit: () => void;
  } = $props();

  let text = $state("");
  let file = $state<File | null>(null);
  let caption = $state("");
  let pollOpen = $state(false);
  let pollQuestion = $state("");
  let pollOptions = $state(["", ""]);
  let pollMulti = $state(false);
  let fileInput: HTMLInputElement | undefined = $state();
  const canSend = $derived(connected && canPost && !busy);
  const filledPollOptions = $derived(pollOptions.map((option) => option.trim()).filter(Boolean));
  const validPoll = $derived(pollQuestion.trim().length > 0 && filledPollOptions.length >= 2
    && new Set(filledPollOptions).size === filledPollOptions.length);

  $effect(() => {
    void account; void generation; void chat;
    text = editing?.text ?? "";
    file = null;
    caption = "";
    pollOpen = false;
  });

  async function send() {
    if (!canSend) return;
    const scope = { account, generation, chat, editing };
    const sent = scope.editing
      ? await onedit(scope.editing.id, text)
      : file ? await onmedia(file, caption) : await ontext(text);
    if (!sent || account !== scope.account || generation !== scope.generation || chat !== scope.chat || editing !== scope.editing) return;
    text = "";
    file = null;
    caption = "";
    if (editing) oncancelEdit();
  }

  async function sendPoll() {
    if (!canSend || !validPoll) return;
    const scope = { account, generation, chat };
    if (!await onpoll(pollQuestion, filledPollOptions, pollMulti)
      || account !== scope.account || generation !== scope.generation || chat !== scope.chat) return;
    pollQuestion = "";
    pollOptions = ["", ""];
    pollMulti = false;
    pollOpen = false;
  }

  function selectFile(event: Event) {
    file = (event.currentTarget as HTMLInputElement).files?.[0] ?? null;
    if (fileInput) fileInput.value = "";
  }
</script>

{#if canPost}
  <section class="channel-composer" aria-label={t("channels.composer")}>
    {#if error}<p class="error" role="alert">{error.message}</p>
      {#if error.diagnostic}<details><summary>{t("error.technical_details")}</summary><pre dir="auto">{error.diagnostic}</pre></details>{/if}
    {/if}
    {#if pollOpen}
      <div class="poll-form">
        <label class="field-label">{t("channels.poll_question")}
          <input class="field" maxlength="255" dir="auto" bind:value={pollQuestion} disabled={!canSend} />
        </label>
        {#each pollOptions as option, index (index)}
          <div class="poll-option">
            <input class="field" maxlength="100" dir="auto" value={option} disabled={!canSend}
              aria-label={t("channels.poll_option", { count: index + 1 })}
              oninput={(event) => { pollOptions = pollOptions.map((value, i) => i === index ? event.currentTarget.value : value); }} />
            {#if pollOptions.length > 2}
              <Button variant="icon" icon="x" title={t("channels.remove_poll_option", { count: index + 1 })}
                aria-label={t("channels.remove_poll_option", { count: index + 1 })} disabled={!canSend}
                onclick={() => { pollOptions = pollOptions.filter((_, i) => i !== index); }} />
            {/if}
          </div>
        {/each}
        <div class="poll-actions">
          <Button variant="ghost" disabled={!canSend || pollOptions.length >= 12} onclick={() => (pollOptions = [...pollOptions, ""])}>{t("channels.add_poll_option")}</Button>
          <label class="multi"><input type="checkbox" bind:checked={pollMulti} disabled={!canSend} />{t("channels.poll_multiple")}</label>
          <Button variant="ghost" disabled={!canSend} onclick={() => (pollOpen = false)}>{t("ui.cancel")}</Button>
          <Button variant="primary" disabled={!canSend || !validPoll} onclick={() => void sendPoll()}>{t("channels.publish_poll")}</Button>
        </div>
      </div>
    {:else}
      {#if editing}<p class="editing">{t("channels.editing_post")}</p>{/if}
      {#if file}
        <div class="file-row"><span>{file.name}</span><Button variant="ghost" disabled={!canSend} onclick={() => (file = null)}>{t("channels.remove_media")}</Button></div>
        <label class="field-label">{t("channels.media_caption")}
          <textarea class="field" rows="2" maxlength="4096" dir="auto" bind:value={caption} disabled={!canSend}></textarea>
        </label>
      {:else}
        <textarea class="field text" rows="2" maxlength="65536" dir="auto" bind:value={text}
          placeholder={t("channels.post_placeholder")} disabled={!canSend}></textarea>
      {/if}
      <div class="actions">
        <Button variant="ghost" disabled={!canSend || !!editing || !!file} onclick={() => (fileInput?.click())}>{t("channels.attach_media")}</Button>
        <Button variant="ghost" disabled={!canSend || !!editing || !!file} onclick={() => (pollOpen = true)}>{t("channels.create_poll")}</Button>
        {#if editing}<Button variant="ghost" disabled={busy} onclick={oncancelEdit}>{t("ui.cancel")}</Button>{/if}
        <Button variant="primary" disabled={!canSend || (file ? false : !text.trim())} onclick={() => void send()}>
          {busy ? t("channels.posting") : editing ? t("channels.save_post") : t("channels.publish")}
        </Button>
      </div>
    {/if}
    <input class="file-input" bind:this={fileInput} type="file" disabled={!canSend} onchange={selectFile} />
  </section>
{/if}

<style>
  .channel-composer { flex: none; display: grid; gap: 8px; padding: 10px 16px; border-top: 1px solid var(--line); background: var(--surface); }
  .field { width: 100%; box-sizing: border-box; padding: 8px 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--bg); color: var(--text); font: inherit; }
  .text { resize: vertical; min-height: 48px; max-height: 180px; }
  .field-label { display: grid; gap: 5px; font-size: .8rem; color: var(--muted); }
  .actions, .poll-actions, .file-row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .actions { justify-content: end; }
  .poll-option { display: flex; align-items: center; gap: 6px; }
  .poll-option .field { flex: 1; }
  .poll-form { display: grid; gap: 8px; }
  .poll-actions { justify-content: end; }
  .multi { margin-inline-end: auto; display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: .8rem; }
  .file-row span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .editing { margin: 0; color: var(--muted); font-size: .8rem; }
  .error { margin: 0; color: var(--danger); overflow-wrap: anywhere; }
  .file-input { display: none; }
</style>
