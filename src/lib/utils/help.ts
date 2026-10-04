const KEY = "postal.helpDismissed";

export function helpDismissed(storage?: Pick<Storage, "getItem">): boolean {
  try {
    return (storage ?? (typeof localStorage === "undefined" ? undefined : localStorage))?.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function dismissHelp(storage?: Pick<Storage, "setItem">): void {
  try {
    (storage ?? (typeof localStorage === "undefined" ? undefined : localStorage))?.setItem(KEY, "1");
  } catch {}
}

export function helpShortcut(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "isComposing" | "defaultPrevented">,
  enabled: boolean,
  editable: boolean,
): boolean {
  return enabled && !editable && !event.isComposing && !event.defaultPrevented &&
    !event.ctrlKey && !event.metaKey && !event.altKey && event.key === "?";
}
