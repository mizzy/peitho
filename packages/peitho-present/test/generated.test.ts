import { describe, expect, it } from "vitest";
import type { Manifest } from "../../../bindings/Manifest";
import type {
  PresentationEndDetail,
  PresentationStartDetail,
  PresenterOptions,
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
});
