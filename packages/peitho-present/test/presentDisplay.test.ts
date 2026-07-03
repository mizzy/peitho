import { afterEach, expect, it, vi } from "vitest";
import {
  buildPresenterFeatures,
  chooseOtherScreen,
  openPresenterWithDisplay,
  placeWindows,
  showPlacementOverlay
} from "../src/presentDisplay";

afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

it("compiles the minimal window management types", () => {
  const screen: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const details: ScreenDetails = {
    currentScreen: screen,
    screens: [screen]
  };
  const options: FullscreenOptions = { screen };

  expect(details.currentScreen.availLeft).toBe(1440);
  expect(options.screen).toBe(screen);
});

it("builds presenter popup features from the current screen", () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };

  expect(buildPresenterFeatures(current)).toBe("popup=yes,width=1200,height=800,left=0,top=0");
});

it("caps presenter popup size to the current screen", () => {
  const current: ScreenDetailed = {
    availLeft: 20,
    availTop: 30,
    availWidth: 900,
    availHeight: 700
  };

  expect(buildPresenterFeatures(current)).toBe("popup=yes,width=900,height=700,left=20,top=30");
});

it("chooses a non-current screen when one exists", () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };

  expect(chooseOtherScreen({ currentScreen: current, screens: [current, other] })).toBe(other);
});

it("places the slide fullscreen on the other screen and popup on the current screen", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const requestFullscreen = vi.fn(async () => undefined);

  await placeWindows({
    details: { currentScreen: current, screens: [current, other] },
    popup,
    requestFullscreen
  });

  expect(requestFullscreen).toHaveBeenCalledWith({ screen: other });
  expect(popup.moveTo).toHaveBeenCalledWith(0, 0);
  expect(popup.resizeTo).toHaveBeenCalledWith(1200, 800);
});

it("does nothing when there is no other screen", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const requestFullscreen = vi.fn();

  await placeWindows({
    details: { currentScreen: current, screens: [current] },
    popup,
    requestFullscreen
  });

  expect(requestFullscreen).not.toHaveBeenCalled();
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(popup.resizeTo).not.toHaveBeenCalled();
});

it("shows a visible placement overlay and invokes the retry on click", async () => {
  const retry = vi.fn(async () => undefined);
  const overlay = showPlacementOverlay(document, retry);
  const button = document.querySelector<HTMLButtonElement>("[data-peitho-place-overlay]");

  expect(button).not.toBeNull();
  expect(button?.textContent).toContain("Click to place windows");
  expect(button?.textContent).toContain("クリックで画面を配置");

  button?.click();
  await Promise.resolve();

  expect(retry).toHaveBeenCalledTimes(1);
  overlay.remove();
  expect(document.querySelector("[data-peitho-place-overlay]")).toBeNull();
});

it("opens the popup before awaiting screen details, then places windows on two screens", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  let resolveDetails: (details: ScreenDetails) => void = () => {};
  const detailsPromise = new Promise<ScreenDetails>((resolve) => {
    resolveDetails = resolve;
  });
  const openWindow = vi.fn(() => popup);
  const requestFullscreen = vi.fn(async () => undefined);

  const pending = openPresenterWithDisplay({
    getScreenDetails: () => detailsPromise,
    openWindow,
    requestFullscreen
  });

  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
  expect(requestFullscreen).not.toHaveBeenCalled();

  resolveDetails({ currentScreen: current, screens: [current, other] });
  await pending;

  expect(requestFullscreen).toHaveBeenCalledWith({ screen: other });
  expect(popup.moveTo).toHaveBeenCalledWith(0, 0);
  expect(popup.resizeTo).toHaveBeenCalledWith(1200, 800);
});

it("shows overlay after NotAllowedError and retries placement from the overlay click", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const requestFullscreen = vi.fn(async (_options?: FullscreenOptions) => undefined);
  requestFullscreen
    .mockRejectedValueOnce(new DOMException("activation expired", "NotAllowedError"))
    .mockResolvedValueOnce(undefined);
  const captured: { retry?: () => Promise<void> } = {};
  const overlay = { remove: vi.fn() };
  const showPlacementOverlay = vi.fn((nextRetry: () => Promise<void>) => {
    captured.retry = nextRetry;
    return overlay;
  });

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    openWindow: vi.fn(() => popup),
    requestFullscreen,
    showPlacementOverlay
  });

  expect(showPlacementOverlay).toHaveBeenCalledTimes(1);
  expect(popup.moveTo).not.toHaveBeenCalled();
  const retry = captured.retry;
  expect(retry).not.toBeNull();
  if (!retry) throw new Error("retry was not captured");

  await retry();

  expect(requestFullscreen).toHaveBeenNthCalledWith(2, { screen: other });
  expect(popup.moveTo).toHaveBeenCalledWith(0, 0);
  expect(popup.resizeTo).toHaveBeenCalledWith(1200, 800);
  expect(overlay.remove).toHaveBeenCalledTimes(1);
});

it("does not show overlay when the first placement succeeds", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const showPlacementOverlay = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    openWindow: vi.fn(() => popup),
    requestFullscreen: vi.fn(async () => undefined),
    showPlacementOverlay
  });

  expect(showPlacementOverlay).not.toHaveBeenCalled();
  expect(popup.moveTo).toHaveBeenCalledWith(0, 0);
});

it("does not show overlay when popup is blocked", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const showPlacementOverlay = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    openWindow: vi.fn(() => null),
    requestFullscreen: vi.fn(async () => {
      throw new DOMException("activation expired", "NotAllowedError");
    }),
    showPlacementOverlay
  });

  expect(showPlacementOverlay).not.toHaveBeenCalled();
});

it("falls back to popup only when the api is absent", async () => {
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const openWindow = vi.fn(() => popup);
  const requestFullscreen = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: undefined,
    openWindow,
    requestFullscreen
  });

  expect(requestFullscreen).not.toHaveBeenCalled();
  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(popup.resizeTo).not.toHaveBeenCalled();
});

it("falls back to popup only for a single screen", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const openWindow = vi.fn(() => popup);
  const requestFullscreen = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current] }),
    openWindow,
    requestFullscreen
  });

  expect(requestFullscreen).not.toHaveBeenCalled();
  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(popup.resizeTo).not.toHaveBeenCalled();
});

it("falls back to popup only when permission is rejected", async () => {
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const openWindow = vi.fn(() => popup);
  const requestFullscreen = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: async () => {
      throw new Error("permission denied");
    },
    openWindow,
    requestFullscreen
  });

  expect(requestFullscreen).not.toHaveBeenCalled();
  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(popup.resizeTo).not.toHaveBeenCalled();
});

it("skips popup placement when the popup is blocked even after the synchronous open", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const openWindow = vi.fn(() => null);
  const requestFullscreen = vi.fn(async () => undefined);

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    openWindow,
    requestFullscreen
  });

  expect(requestFullscreen).toHaveBeenCalledWith({ screen: other });
  expect(openWindow).toHaveBeenCalledTimes(1);
});
