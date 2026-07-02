import type { NavigateTarget } from "./shell";

const keyMap = new Map<string, NavigateTarget>([
  ["ArrowRight", "next"],
  ["PageDown", "next"],
  [" ", "next"],
  ["ArrowLeft", "prev"],
  ["PageUp", "prev"],
  ["Home", "first"],
  ["End", "last"]
]);

export function installKeyboardNavigation(
  win: Window = window,
  bus: EventTarget = win
): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    const to = keyMap.get(event.key);
    if (!to) return;
    event.preventDefault();
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
