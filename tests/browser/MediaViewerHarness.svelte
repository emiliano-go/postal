<script lang="ts">
  import MediaViewer, { type MediaViewerAction, type ViewerItem } from "$lib/media/MediaViewer.svelte";
  import { accessibility } from "$lib/utils/accessibility.svelte";

  const svg = (fill: string) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 420"><rect width="640" height="420" fill="${fill}"/><text x="24" y="210" fill="white" font-size="36">Synthetic media</text></svg>`)}`;
  const items: ViewerItem[] = [
    { id: "image-1", path: svg("#276c91"), thumb: svg("#276c91"), kind: "image", caption: "First synthetic caption", author: "Fixture sender", avatar: null, timestamp: 1791100000 },
    { id: "image-2", path: svg("#856629"), thumb: svg("#856629"), kind: "image", caption: "Second caption", author: "Fixture sender", avatar: null, timestamp: 1791100060 },
    { id: "video-1", path: "F:/synthetic/viewer.mp4", thumb: svg("#542c76"), kind: "video", caption: "Synthetic video caption", author: "Fixture sender", avatar: null, timestamp: 1791100120 },
    { id: "document-1", path: svg("#43593c"), thumb: svg("#43593c"), kind: "document", caption: "Synthetic document", author: "Fixture sender", avatar: null, timestamp: 1791100180 },
  ];
  let index = $state(0);
  let open = $state(true);
  let viewOnce = $state(false);
  let reducedMotion = $state(false);
  let actions = $state<string[]>([]);
  let focusTarget = $state("none");
  let lastEvent = $state("none");
  const mediaAction = (id: string, action: MediaViewerAction) => { actions = [...actions, `${id}:${action}`]; };
  function toggleReducedMotion() {
    reducedMotion = !reducedMotion;
    accessibility.reduceMotion = reducedMotion ? "on" : "off";
  }
</script>

<svelte:window onfocusin={(event) => (focusTarget = (event.target as HTMLElement).getAttribute("aria-label") ?? (event.target as HTMLElement).tagName)} />

<main>
  <h1>Media viewer interaction fixture</h1>
  <button onclick={() => (open = true)}>Open viewer</button>
  <label><input type="checkbox" bind:checked={viewOnce} /> View-once mode</label>
  <button onclick={toggleReducedMotion}>Toggle reduced motion ({reducedMotion ? "on" : "off"})</button>
  <output aria-label="Media actions">{actions.join(", ") || "none"}</output>
  <output aria-label="Opened media">{lastEvent}</output>
  <output aria-label="Focused element">{focusTarget}</output>
</main>

{#if open}
  <MediaViewer
    items={items}
    bind:index
    onclose={() => { lastEvent = "closed"; open = false; }}
    onopen={viewOnce ? undefined : (path) => (lastEvent = `open:${path}`)}
    onmediaaction={viewOnce ? undefined : mediaAction}
    onreply={(id) => (lastEvent = `reply:${id}`)}
    onjump={(id) => (lastEvent = `jump:${id}`)} />
{/if}

<style>
  :global(:root) { --chat-bg: #172126; --text: #e9edef; --muted: #aebac1; --raised-2: #374248; --surface: #263238; --raised: #334047; --line-strong: #52616a; --shadow: 0 8px 24px #0008; --radius-lg: 12px; --radius-sm: 5px; --accent: #00a884; --motion-scale: 1; --ease: ease; }
  :global(body) { margin: 0; font: 16px system-ui; color: var(--text); background: var(--chat-bg); }
  main { display: flex; gap: 12px; align-items: center; padding: 12px; }
  h1 { font-size: 16px; margin: 0; }
  output { position: fixed; left: 12px; bottom: 8px; z-index: 500; background: #111e; padding: 4px 8px; }
  output[aria-label="Opened media"] { bottom: 34px; }
  output[aria-label="Focused element"] { bottom: 60px; }
</style>
