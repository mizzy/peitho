import { expect, it, vi } from "vitest";
import {
  buildPresenterFeatures,
  chooseOtherScreen,
  openPresenterWithDisplay
} from "../src/presentDisplay";

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
