<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import Button from "$lib/ui/Button.svelte";
  import Dialog from "$lib/ui/Dialog.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import ContactEditor from "$lib/contacts/ContactEditor.svelte";
  import GroupCreator from "./GroupCreator.svelte";
  import type { GroupCreateResult, SearchResult } from "$lib/utils/wire";

  let {
    account, me, avatars, connected, onavatar, onsearch, oncreate, onopen, onsaved, onclose,
  }: {
    account: string;
    me: string | null;
    avatars: Record<string, string | null>;
    connected: boolean;
    onavatar: (jid: string) => void;
    onsearch: (query: string) => Promise<SearchResult[]>;
    oncreate: (subject: string, jids: string[]) => Promise<GroupCreateResult>;
    onopen: (result: GroupCreateResult) => Promise<void>;
    onsaved: (jid: string) => void;
    onclose: () => void;
  } = $props();

  let tab = $state<"contact" | "group">("contact");
  let groupBusy = $state(false);
  function close() {
    if (!groupBusy) onclose();
  }
</script>

<Dialog size="sm" style="--dialog-width: min(440px, calc(100vw - 32px)); max-height: min(680px, calc(100vh - 64px)); padding: 0; background: var(--bg);"
  label={t("chat.new_chat")} open onclose={close}>
  <div class="body">
    <header>
      <h2>{t("chat.new_chat")}</h2>
      <button class="close" type="button" aria-label={t("ui.close")} disabled={groupBusy} onclick={close}><Icon name="x" size={18} /></button>
    </header>
    <div class="tabs" role="tablist" aria-label={t("chat.new_chat")}>
      <Button variant="chip" selected={tab === "contact"} onclick={() => (tab = "contact")}>{t("contact.new")}</Button>
      <Button variant="chip" selected={tab === "group"} onclick={() => (tab = "group")}>{t("group.new")}</Button>
    </div>
    <div role="tabpanel" hidden={tab !== "contact"}>
      <ContactEditor {account} {connected} onsaved={onsaved} />
    </div>
    <div role="tabpanel" hidden={tab !== "group"}>
      <GroupCreator {account} {me} {avatars} {onavatar} {onsearch} {oncreate} {onopen} onclose={close} bind:busy={groupBusy} />
    </div>
  </div>
</Dialog>

<style>
  .body { display: flex; flex-direction: column; gap: 10px; padding: 18px 20px; }
  header { display: flex; align-items: center; justify-content: space-between; }
  h2 { margin: 0; font-size: 1.0625rem; font-weight: 600; }
  .close { display: grid; place-items: center; width: 32px; height: 32px; border: 0; border-radius: 50%; background: transparent; color: var(--muted); cursor: pointer; }
  .close:hover { background: var(--raised); color: var(--text); }
  .tabs { display: flex; gap: 8px; }
  .close:disabled { opacity: 0.5; cursor: default; }
</style>
