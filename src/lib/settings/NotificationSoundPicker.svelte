<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { invoke } from "$lib/utils/ipc";
  import { playNotificationSound } from "$lib/utils/notification-sound";
  import type { NotificationSound } from "$lib/utils/wire";

  let { value, previewSound, allowInherit = false, onchange }: {
    value: NotificationSound | null;
    previewSound: NotificationSound;
    allowInherit?: boolean;
    onchange: (value: NotificationSound | null) => void;
  } = $props();
  let previewing = $state(false);
  let previewResult = $state<"played" | "unavailable" | "failed" | null>(null);

  async function preview() {
    if (previewing) return;
    previewing = true;
    previewResult = null;
    try {
      const played = previewSound === "system"
        ? await invoke<boolean>("preview_notification_sound")
        : await playNotificationSound(previewSound);
      previewResult = played ? "played" : previewSound === "system" ? "unavailable" : "failed";
    } catch {
      previewResult = "failed";
    }
    previewing = false;
  }

  function change(sound: string) {
    previewResult = null;
    onchange((sound || null) as NotificationSound | null);
  }
</script>

<label class="field-label">
  <span>{t("settings.sound_choice")}</span>
  <select class="field" value={value ?? ""} onchange={(event) => change(event.currentTarget.value)}>
    {#if allowInherit}<option value="">{t("settings.sound_inherit")}</option>{/if}
    <option value="system">{t("settings.sound_system")}</option>
    <option value="chime">{t("settings.sound_chime")}</option>
    <option value="pop">{t("settings.sound_pop")}</option>
    <option value="soft">{t("settings.sound_soft")}</option>
  </select>
</label>
<div class="sound-actions">
  <button class="button" type="button" disabled={previewing} onclick={preview}>
    {previewing ? t("settings.sound_previewing") : t("settings.sound_preview")}
  </button>
  {#if previewResult === "played"}<span class="hint" role="status" data-preview-result="played">{t("settings.sound_preview_played")}</span>{/if}
  {#if previewResult === "unavailable" && previewSound === "system"}<span class="hint">{t("settings.sound_system_preview_hint")}</span>{/if}
  {#if previewResult === "failed"}<span class="hint" role="alert">{t("settings.sound_preview_failed")}</span>{/if}
</div>

<style>
  .sound-actions { display: flex; align-items: center; gap: .75rem; flex-wrap: wrap; }
  .hint { color: var(--muted); font-size: .8rem; }
</style>
