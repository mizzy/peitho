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

export type OpenPresenterWithDisplayOptions = {
  window?: Window;
  document?: Document;
  url?: string;
  getScreenDetails?: (() => Promise<ScreenDetails>) | undefined;
  openWindow?: (url: string, target: string, features: string) => PresenterPopup | null;
  requestFullscreen?: (options?: FullscreenOptions) => Promise<void> | void;
};

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

  if (!getScreenDetails) return popup;

  try {
    const details = await getScreenDetails();
    const otherScreen = chooseOtherScreen(details);
    if (otherScreen) {
      await requestFullscreen({ screen: otherScreen });
      if (popup) {
        popup.moveTo(details.currentScreen.availLeft, details.currentScreen.availTop);
        popup.resizeTo(
          Math.min(DEFAULT_POPUP_WIDTH, details.currentScreen.availWidth),
          Math.min(DEFAULT_POPUP_HEIGHT, details.currentScreen.availHeight)
        );
      }
    }
  } catch {
    return popup;
  }

  return popup;
}
