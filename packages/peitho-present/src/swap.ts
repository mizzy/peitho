import { hasChordModifier } from "./keyboard";

export type SwapRoute = Readonly<{ swapped: boolean; counterpart: string }>;

const SWAP_ROUTES: Readonly<Record<string, SwapRoute>> = Object.freeze({
  "/present.html": Object.freeze({ swapped: false, counterpart: "presenter-swapped" }),
  "/": Object.freeze({ swapped: false, counterpart: "presenter-swapped" }),
  "/presenter": Object.freeze({ swapped: false, counterpart: "present-swapped" }),
  "/presenter.html": Object.freeze({ swapped: false, counterpart: "present-swapped" }),
  "/present-swapped": Object.freeze({ swapped: true, counterpart: "presenter" }),
  "/presenter-swapped": Object.freeze({ swapped: true, counterpart: "present.html" })
});

export function swapRoute(pathname: string): SwapRoute | null {
  return SWAP_ROUTES[pathname] ?? null;
}

export function installSwapShortcut(win: Window = window, bus: EventTarget = win): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    if (hasChordModifier(event)) return;
    if (event.key !== "s" && event.key !== "S") return;
    if (event.repeat) return;
    event.preventDefault();
    bus.dispatchEvent(new CustomEvent("peitho:swaprequest"));
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
