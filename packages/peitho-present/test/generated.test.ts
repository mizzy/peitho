import { describe, expect, it } from "vitest";
import type { Manifest } from "../../../bindings/Manifest";
import type { ShellOptions } from "../src/index";

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
});
