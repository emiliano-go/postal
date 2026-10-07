<script lang="ts">
  import NotificationSoundPicker from "../../src/lib/settings/NotificationSoundPicker.svelte";
  import type { NotificationSound } from "../../src/lib/utils/wire";

  let sound = $state<NotificationSound>("chime");
</script>

<main>
  <h1>Notification sound preview</h1>
  <p>No account or native notification is used by this fixture.</p>
  <NotificationSoundPicker value={sound} previewSound={sound} onchange={(next) => { if (next) sound = next; }} />
</main>
