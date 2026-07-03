export const PRESENTER_URL = "presenter.html";
export const PRESENTER_TARGET = "peitho-presenter";

export const DEFAULT_POPUP_WIDTH = 1200;
export const DEFAULT_POPUP_HEIGHT = 800;

export function buildPresenterFeatures(screen: ScreenDetailed): string {
  const width = Math.min(DEFAULT_POPUP_WIDTH, screen.availWidth);
  const height = Math.min(DEFAULT_POPUP_HEIGHT, screen.availHeight);
  return [
    "popup=yes",
    `width=${width}`,
    `height=${height}`,
    `left=${screen.availLeft}`,
    `top=${screen.availTop}`
  ].join(",");
}

export function fallbackFeatures(): string {
  return "popup=yes,width=1200,height=800,left=80,top=80";
}

export function chooseOtherScreen(details: ScreenDetails): ScreenDetailed | null {
  return details.screens.find((screen) => screen !== details.currentScreen) ?? null;
}

export type PresenterPopup = Pick<Window, "moveTo" | "resizeTo">;
export type RequestFullscreen = (options?: FullscreenOptions) => Promise<void> | void;

export type PlaceWindowsOptions = {
  details: ScreenDetails;
  popup: PresenterPopup | null;
  requestFullscreen: RequestFullscreen;
};

export type PlacementOverlay = {
  remove: () => void;
};

export type ShowPlacementOverlay = (retry: () => Promise<void>) => PlacementOverlay;

export async function placeWindows(options: PlaceWindowsOptions): Promise<boolean> {
  const otherScreen = chooseOtherScreen(options.details);
  if (!otherScreen) return false;

  await options.requestFullscreen({ screen: otherScreen });

  if (options.popup) {
    options.popup.moveTo(
      options.details.currentScreen.availLeft,
      options.details.currentScreen.availTop
    );
    options.popup.resizeTo(
      Math.min(DEFAULT_POPUP_WIDTH, options.details.currentScreen.availWidth),
      Math.min(DEFAULT_POPUP_HEIGHT, options.details.currentScreen.availHeight)
    );
  }

  return true;
}

export function showPlacementOverlay(
  doc: Document,
  retry: () => Promise<void>
): PlacementOverlay {
  const button = doc.createElement("button");
  button.type = "button";
  button.dataset.peithoPlaceOverlay = "true";
  button.textContent = "Click to place windows / クリックで画面を配置";
  button.style.position = "fixed";
  button.style.inset = "0";
  button.style.zIndex = "2147483647";
  button.style.display = "grid";
  button.style.placeItems = "center";
  button.style.border = "0";
  button.style.background = "rgba(0, 0, 0, 0.82)";
  button.style.color = "#fff";
  button.style.font = "600 28px system-ui, sans-serif";
  button.addEventListener("click", () => {
    void retry();
  });
  doc.body.appendChild(button);

  return {
    remove: () => button.remove()
  };
}

export type OpenPresenterWithDisplayOptions = {
  window?: Window;
  document?: Document;
  url?: string;
  getScreenDetails?: (() => Promise<ScreenDetails>) | undefined;
  openWindow?: (url: string, target: string, features: string) => PresenterPopup | null;
  requestFullscreen?: RequestFullscreen;
  showPlacementOverlay?: ShowPlacementOverlay;
};

function isNotAllowedError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "NotAllowedError";
}

export async function openPresenterWithDisplay(
  options: OpenPresenterWithDisplayOptions = {}
): Promise<PresenterPopup | null> {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const url = options.url ?? PRESENTER_URL;
  const openWindow =
    options.openWindow ?? ((nextUrl, target, features) => win.open(nextUrl, target, features));
  const popup = openWindow(url, PRESENTER_TARGET, fallbackFeatures());
  const requestFullscreen =
    options.requestFullscreen ??
    ((fullscreenOptions?: FullscreenOptions) =>
      doc.documentElement.requestFullscreen?.(fullscreenOptions));
  const getScreenDetails = options.getScreenDetails ?? win.getScreenDetails?.bind(win);
  const showOverlay =
    options.showPlacementOverlay ??
    ((retry: () => Promise<void>) => showPlacementOverlay(doc, retry));

  if (!getScreenDetails) return popup;

  let details: ScreenDetails;
  try {
    details = await getScreenDetails();
  } catch {
    return popup;
  }

  try {
    await placeWindows({ details, popup, requestFullscreen });
  } catch (error) {
    if (!popup || !isNotAllowedError(error)) return popup;
    let overlay: PlacementOverlay | null = null;
    overlay = showOverlay(async () => {
      try {
        await placeWindows({ details, popup, requestFullscreen });
        overlay?.remove();
        overlay = null;
      } catch {
        return;
      }
    });
  }

  return popup;
}
