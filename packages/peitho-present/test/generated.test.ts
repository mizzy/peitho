import { describe, expect, it } from "vitest";
import type { Manifest } from "../../../bindings/Manifest";
import type { PresentConfig } from "../../../bindings/PresentConfig";
import {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  calculateCanvasFit,
  fallbackFeatures,
  installCanvasClickNavigation,
  installCanvasScaler,
  installCloseOnEscape,
  installFullscreenShortcut,
  installPresentationControls,
  mountPresenterView,
  mountPresentShell,
  openPresenterPopup,
  serverSyncChannelFactory
} from "../src/index";
import type {
  OpenPresenterPopupOptions,
  PresentationEndDetail,
  PresentationStartDetail,
  PresenterOptions,
  ServerSyncOptions,
  ShellOptions,
  TimerControlDetail
} from "../src/index";

describe("generated manifest contract", () => {
  it("uses the Rust-generated Manifest type shape", () => {
    const manifest: Manifest = {
      version: 1,
      peithoVersion: "0.1.0",
      title: "Demo",
      slideCount: 1,
      plannedDurationMs: null,
      sections: [],
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

  it("uses the Rust-generated PresentConfig type shape", () => {
    const config: PresentConfig = { version: 1, presenterOpen: true };

    expect(config.version).toBe(1);
    expect(config.presenterOpen).toBe(true);
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
    expect(typeof installCloseOnEscape).toBe("function");
    expect(typeof installFullscreenShortcut).toBe("function");
    expect(typeof mountPresentShell).toBe("function");
    expect(typeof mountPresenterView).toBe("function");
  });

  it("exports presenter popup and server sync helpers", () => {
    const popupOptions: OpenPresenterPopupOptions = {
      url: "presenter.html",
      openWindow: () => null
    };
    const syncOptions: ServerSyncOptions = {
      url: "/sync"
    };

    expect(fallbackFeatures()).toContain("popup=yes");
    expect(typeof openPresenterPopup).toBe("function");
    expect(typeof serverSyncChannelFactory).toBe("function");
    expect(popupOptions.url).toBe("presenter.html");
    expect(syncOptions.url).toBe("/sync");
  });
});
