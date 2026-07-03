import { describe, expect, it } from "vitest";
import type { Manifest } from "../../../bindings/Manifest";
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  buildPresenterFeatures,
  calculateCanvasFit,
  chooseOtherScreen,
  installCanvasClickNavigation,
  installCanvasScaler,
  installFullscreenShortcut,
  installPresentationControls,
  mountPresenterView,
  mountPresentShell,
  openPresenterWithDisplay,
  placeWindows,
  showPlacementOverlay
} from "../src/index";
import type {
  PlacementOverlay,
  PlaceWindowsOptions,
  PresentationEndDetail,
  PresentationStartDetail,
  PresenterOptions,
  RequestFullscreen,
  ShellOptions,
  ShowPlacementOverlay,
  TimerControlDetail
} from "../src/index";

describe("generated manifest contract", () => {
  it("uses the Rust-generated Manifest type shape", () => {
    const manifest: Manifest = {
      version: 1,
      peithoVersion: "0.1.0",
      title: "Demo",
      slideCount: 1,
      slides: [
        {
          index: 0,
          key: "intro",
          src: "slides/000-intro.html",
          hasNotes: false
        }
      ]
    };
    const options: ShellOptions = { root: document.createElement("main") };

    expect(manifest.slides[0].key).toBe("intro");
    expect(options.root.tagName).toBe("MAIN");
  });

  it("exports presenter and presentation event types", () => {
    const start: PresentationStartDetail = { total: 3, startedAt: 1000 };
    const end: PresentationEndDetail = { endedAt: 2000, elapsedMs: 1000 };
    const control: TimerControlDetail = { action: "pause" };
    const options: Pick<PresenterOptions, "root" | "notes"> = {
      root: document.createElement("main"),
      notes: { version: 1, notes: {} }
    };

    expect(start.total).toBe(3);
    expect(end.elapsedMs).toBe(1000);
    expect(control.action).toBe("pause");
    expect(options.notes.version).toBe(1);
  });

  it("exports the presentation canvas public API", () => {
    expect(CANVAS_WIDTH).toBe(1280);
    expect(CANVAS_HEIGHT).toBe(720);
    expect(calculateCanvasFit({ width: 1280, height: 720 }).scale).toBe(1);
    expect(typeof installCanvasScaler).toBe("function");
    expect(typeof installPresentationControls).toBe("function");
    expect(typeof installCanvasClickNavigation).toBe("function");
    expect(typeof installFullscreenShortcut).toBe("function");
    expect(typeof mountPresentShell).toBe("function");
    expect(typeof mountPresenterView).toBe("function");
  });

  it("exports display management helpers", () => {
    expect(typeof buildPresenterFeatures).toBe("function");
    expect(typeof chooseOtherScreen).toBe("function");
    expect(typeof openPresenterWithDisplay).toBe("function");
  });

  it("exports display placement retry helpers", () => {
    const overlay: PlacementOverlay = { remove: () => undefined };
    const fullscreen: RequestFullscreen = () => undefined;
    const show: ShowPlacementOverlay = () => overlay;
    const options: PlaceWindowsOptions = {
      details: {
        currentScreen: { availLeft: 0, availTop: 0, availWidth: 1, availHeight: 1 },
        screens: [{ availLeft: 0, availTop: 0, availWidth: 1, availHeight: 1 }]
      },
      popup: null,
      requestFullscreen: fullscreen
    };

    expect(typeof placeWindows).toBe("function");
    expect(typeof showPlacementOverlay).toBe("function");
    expect(show).toBeTypeOf("function");
    expect(options.popup).toBeNull();
  });
});
