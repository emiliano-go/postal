export type MessageRailAction = "previous" | "next" | "page-previous" | "page-next" | "first" | "last" | "menu" | "reply" | "star" | "select";

export function messageRailId(id: string) {
  return `message-rail-${encodeURIComponent(id)}`;
}

export function messageRailAction(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "defaultPrevented" | "isComposing">,
  editable = false,
  charShortcutsEnabled = true,
): MessageRailAction | null {
  if (editable || event.defaultPrevented || event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return null;
  switch (event.key) {
    case "ArrowUp": return "previous";
    case "ArrowDown": return "next";
    case "PageUp": return "page-previous";
    case "PageDown": return "page-next";
    case "Home": return "first";
    case "End": return "last";
    case "Enter": return "menu";
    case " ":
    case "Spacebar": return "select";
  }
  if (!charShortcutsEnabled) return null;
  return event.key.toLowerCase() === "r" ? "reply" : event.key.toLowerCase() === "s" ? "star" : null;
}
