export type MediaViewerKeyAction = "previous" | "next" | "zoom-in" | "zoom-out" | "zoom-reset" | "save";

export function nextSlideshowIndex(items: readonly { kind: string }[], index: number, reducedMotion: boolean): number | null {
  if (reducedMotion || items.filter((item) => item.kind !== "audio").length < 2 || index < 0 || index >= items.length - 1) return null;
  for (let next = index + 1; next < items.length; next++) if (items[next].kind !== "audio") return next;
  return null;
}

export function mediaViewerKey(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "defaultPrevented" | "isComposing">,
  canSave: boolean,
): MediaViewerKeyAction | null {
  if (event.defaultPrevented || event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return null;
  if (event.key === "ArrowLeft" && !event.shiftKey) return "previous";
  if (event.key === "ArrowRight" && !event.shiftKey) return "next";
  if (event.key === "+" || event.key === "=") return "zoom-in";
  if (event.key === "-") return "zoom-out";
  if (event.key === "0") return "zoom-reset";
  if (canSave && event.key.toLowerCase() === "s") return "save";
  return null;
}
