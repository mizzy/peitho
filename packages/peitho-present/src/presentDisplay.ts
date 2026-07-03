export const PRESENTER_URL = "presenter.html";
export const PRESENTER_TARGET = "peitho-presenter";

export function fallbackFeatures(): string {
  return "popup=yes,width=1200,height=800,left=80,top=80";
}

export type OpenPresenterPopupOptions = {
  window?: Window;
  url?: string;
  features?: string;
  openWindow?: (url: string, target: string, features: string) => Window | null;
};

export function openPresenterPopup(options: OpenPresenterPopupOptions = {}): Window | null {
  const win = options.window ?? window;
  const url = options.url ?? PRESENTER_URL;
  const features = options.features ?? fallbackFeatures();
  const openWindow =
    options.openWindow ?? ((nextUrl, target, nextFeatures) => win.open(nextUrl, target, nextFeatures));

  return openWindow(url, PRESENTER_TARGET, features);
}
