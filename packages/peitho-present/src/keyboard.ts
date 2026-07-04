import type { NavigateTarget } from "./shell";

const navigationKeyMap = new Map<string, NavigateTarget>([
  ["ArrowRight", "next"],
  ["PageDown", "next"],
  ["ArrowLeft", "prev"],
  ["PageUp", "prev"],
  ["Home", "first"],
  ["End", "last"]
]);

const keyMap = new Map<string, NavigateTarget>([...navigationKeyMap, [" ", "next"]]);

function dispatchNavigate(bus: EventTarget, to: NavigateTarget): void {
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
}

export function installKeyboardNavigation(
  win: Window = window,
  bus: EventTarget = win
): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    const to = keyMap.get(event.key);
    if (!to) return;
    event.preventDefault();
    dispatchNavigate(bus, to);
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}

export function installPresenterKeyboard(
  win: Window,
  bus: EventTarget,
  onPlaypause: () => void
): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    const to = navigationKeyMap.get(event.key);
    if (to) {
      event.preventDefault();
      dispatchNavigate(bus, to);
      return;
    }
    if (event.key !== " ") return;
    event.preventDefault();
    if (event.repeat) return;
    onPlaypause();
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}

export function installCloseOnEscape(win: Window = window, bus: EventTarget = win): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    bus.dispatchEvent(new CustomEvent("peitho:closerequest"));
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
