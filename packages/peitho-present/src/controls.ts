import type { NavigateDetail, SlideChangeDetail } from "./shell";

export type PresentationControlsOptions = {
  root: HTMLElement;
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  idleMs?: number;
  openPresenter?: () => void;
};

export type CanvasClickNavigationOptions = {
  root: HTMLElement;
  window?: Window;
  bus?: EventTarget;
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
    (() => {
      win.open("presenter.html", "_blank", "noopener");
    });

  const bar = doc.createElement("nav");
  bar.dataset.peithoControlBar = "true";
  bar.className = "peitho-control-bar";
  bar.hidden = true;
  bar.innerHTML = [
    '<button type="button" data-peitho-action="prev" aria-label="Previous slide">◀</button>',
    '<button type="button" data-peitho-action="next" aria-label="Next slide">▶</button>',
    '<output data-peitho-control="counter">– / –</output>',
    '<button type="button" data-peitho-action="fullscreen" aria-label="Toggle fullscreen">⛶</button>',
    '<button type="button" data-peitho-action="presenter">Presenter</button>'
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
    if (action === "presenter") openPresenter();
    if (action === "fullscreen") toggleFullscreen(doc);
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
  const onClick = (event: MouseEvent): void => {
    if ((event.target as HTMLElement).closest('[data-peitho-control-bar="true"]')) return;
    const to = event.clientX < win.innerWidth / 4 ? "prev" : "next";
    bus.dispatchEvent(new CustomEvent<NavigateDetail>("peitho:navigate", { detail: { to } }));
  };
  options.root.addEventListener("click", onClick);
  return () => options.root.removeEventListener("click", onClick);
}

export function installFullscreenShortcut(options: FullscreenShortcutOptions = {}): () => void {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const onKeyDown = (event: KeyboardEvent): void => {
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
