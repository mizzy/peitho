import type { NavigateDetail, SlideChangeDetail } from "./shell";
import { createClickNavigationGuard } from "./clickNavigationGuard";
import { hasChordModifier } from "./keyboard";
import { openPresenterPopup, type OpenPresenterPopupOptions } from "./presentDisplay";

export type PresentationControlsOptions = {
  root: HTMLElement;
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  idleMs?: number;
  openPresenter?: () => void | Promise<void>;
  openPresenterWindow?: OpenPresenterPopupOptions["openWindow"];
};

export type CanvasClickNavigationOptions = {
  root: HTMLElement;
  window?: Window;
  bus?: EventTarget;
};

export type SwipeNavigationOptions = {
  root: HTMLElement;
  window?: Window;
  bus?: EventTarget;
  minHorizontalPx?: number;
  maxDurationMs?: number;
  minRatio?: number;
};

export type FullscreenShortcutOptions = {
  window?: Window;
  document?: Document;
};

export function installPresentationControls(options: PresentationControlsOptions): () => void {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
  const idleMs = options.idleMs ?? 3000;
  const openPresenter =
    options.openPresenter ??
    (() =>
      openPresenterPopup({
        window: win,
        openWindow: options.openPresenterWindow
      }));

  const bar = doc.createElement("nav");
  bar.dataset.peithoControlBar = "true";
  bar.className = "peitho-control-bar";
  bar.hidden = true;
  bar.innerHTML = [
    '<button type="button" data-peitho-action="prev" aria-label="Previous slide">◀</button>',
    '<button type="button" data-peitho-action="next" aria-label="Next slide">▶</button>',
    '<output data-peitho-control="counter">– / –</output>',
    '<button type="button" data-peitho-action="fullscreen" aria-label="Toggle fullscreen">⛶</button>',
    '<button type="button" data-peitho-action="presenter">Presenter</button>',
    '<button type="button" data-peitho-action="close" aria-label="Close presentation">✕</button>'
  ].join("");
  options.root.appendChild(bar);

  let hideTimer: number | null = null;
  const clearHideTimer = (): void => {
    if (hideTimer !== null) win.clearTimeout(hideTimer);
    hideTimer = null;
  };
  const show = (): void => {
    bar.hidden = false;
    clearHideTimer();
    hideTimer = win.setTimeout(() => {
      bar.hidden = true;
      hideTimer = null;
    }, idleMs);
  };
  const dispatchNavigate = (to: "prev" | "next"): void => {
    bus.dispatchEvent(new CustomEvent<NavigateDetail>("peitho:navigate", { detail: { to } }));
  };
  const onClick = (event: Event): void => {
    event.stopPropagation();
    const action = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-peitho-action]")
      ?.dataset.peithoAction;
    if (action === "prev" || action === "next") dispatchNavigate(action);
    if (action === "presenter") void openPresenter();
    if (action === "fullscreen") toggleFullscreen(doc);
    if (action === "close") bus.dispatchEvent(new CustomEvent("peitho:closerequest"));
  };
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<SlideChangeDetail>).detail;
    const counter = bar.querySelector<HTMLOutputElement>('[data-peitho-control="counter"]');
    if (counter) counter.textContent = `${detail.index + 1} / ${detail.total}`;
  };

  win.addEventListener("mousemove", show);
  bar.addEventListener("click", onClick);
  bus.addEventListener("peitho:slidechange", onSlideChange);

  return () => {
    clearHideTimer();
    win.removeEventListener("mousemove", show);
    bar.removeEventListener("click", onClick);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bar.remove();
  };
}

export function installCanvasClickNavigation(options: CanvasClickNavigationOptions): () => void {
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const clickGuard = createClickNavigationGuard({ target: options.root, window: win });
  const onClick = (event: MouseEvent): void => {
    if (clickGuard.shouldIgnoreClick(event)) return;
    if ((event.target as HTMLElement).closest('[data-peitho-control-bar="true"]')) return;
    const to = event.clientX < win.innerWidth / 4 ? "prev" : "next";
    bus.dispatchEvent(new CustomEvent<NavigateDetail>("peitho:navigate", { detail: { to } }));
  };
  options.root.addEventListener("click", onClick);
  return () => {
    clickGuard.destroy();
    options.root.removeEventListener("click", onClick);
  };
}

export function installSwipeNavigation(options: SwipeNavigationOptions): () => void {
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const minHorizontalPx = options.minHorizontalPx ?? 50;
  const maxDurationMs = options.maxDurationMs ?? 800;
  const minRatio = options.minRatio ?? 1.5;
  const clickSuppressPx = minHorizontalPx / 2;
  let active = false;
  let x0 = 0;
  let y0 = 0;
  let t0 = 0;

  const onTouchStart = (event: TouchEvent): void => {
    if (active) return;
    if (event.touches.length !== 1) return;
    if ((event.target as HTMLElement).closest('[data-peitho-control-bar="true"]')) return;
    const touch = event.touches[0];
    x0 = touch.clientX;
    y0 = touch.clientY;
    t0 = win.performance.now();
    active = true;
  };
  const onTouchEnd = (event: TouchEvent): void => {
    if (!active) return;
    active = false;
    const touch = event.changedTouches[0];
    if (!touch) return;
    const dx = touch.clientX - x0;
    const dy = touch.clientY - y0;
    const dt = win.performance.now() - t0;
    if (Math.abs(dx) >= clickSuppressPx) event.preventDefault();
    if (Math.abs(dx) < minHorizontalPx) return;
    if (Math.abs(dx) / Math.max(Math.abs(dy), 1) <= minRatio) return;
    if (dt > maxDurationMs) return;
    bus.dispatchEvent(
      new CustomEvent<NavigateDetail>("peitho:navigate", {
        detail: { to: dx < 0 ? "next" : "prev" }
      })
    );
  };
  const onTouchCancel = (): void => {
    active = false;
  };

  options.root.addEventListener("touchstart", onTouchStart, { passive: true });
  options.root.addEventListener("touchend", onTouchEnd, { passive: false });
  options.root.addEventListener("touchcancel", onTouchCancel);

  return () => {
    options.root.removeEventListener("touchstart", onTouchStart);
    options.root.removeEventListener("touchend", onTouchEnd);
    options.root.removeEventListener("touchcancel", onTouchCancel);
  };
}

export function installFullscreenShortcut(options: FullscreenShortcutOptions = {}): () => void {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const onKeyDown = (event: KeyboardEvent): void => {
    if (hasChordModifier(event)) return;
    if (event.key !== "f") return;
    event.preventDefault();
    toggleFullscreen(doc);
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}

export function toggleFullscreen(doc: Document = document): void {
  if (doc.fullscreenElement) {
    void doc.exitFullscreen?.();
    return;
  }
  void doc.documentElement.requestFullscreen?.();
}
