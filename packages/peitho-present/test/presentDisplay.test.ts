import { afterEach, expect, it, vi } from "vitest";
import { fallbackFeatures, openPresenterPopup } from "../src/presentDisplay";

afterEach(() => {
  vi.restoreAllMocks();
});

it("uses a fixed popup feature string for presenter windows", () => {
  expect(fallbackFeatures()).toBe("popup=yes,width=1200,height=800,left=80,top=80");
});

it("opens presenter.html as a named popup window", () => {
  const popup = {} as Window;
  const openWindow = vi.fn(() => popup);

  const result = openPresenterPopup({ openWindow });

  expect(result).toBe(popup);
  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
});

it("allows tests and callers to inject the popup URL and features", () => {
  const openWindow = vi.fn(() => null);

  const result = openPresenterPopup({
    url: "custom-presenter.html",
    features: "popup=yes,width=800,height=600",
    openWindow
  });

  expect(result).toBeNull();
  expect(openWindow).toHaveBeenCalledWith(
    "custom-presenter.html",
    "peitho-presenter",
    "popup=yes,width=800,height=600"
  );
});
