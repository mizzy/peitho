import { expect, it } from "vitest";
import { applyStepState, createStepStateStyle } from "../src/stepState";

it("applies cumulative reveal and equality emphasis in light and shadow DOM", () => {
  const light = document.createElement("section");
  const host = document.createElement("section");
  const shadow = host.attachShadow({ mode: "open" });

  for (const root of [light, shadow]) {
    root.innerHTML =
      '<p data-reveal-step="1"></p><p data-reveal-step="2"></p>' +
      '<span data-emphasis-step="1 2"></span><span data-emphasis-step="2"></span>';

    applyStepState(root, 1);
    const reveals = [...root.querySelectorAll<HTMLElement>("[data-reveal-step]")];
    const emphasis = [...root.querySelectorAll<HTMLElement>("[data-emphasis-step]")];
    expect(reveals.map((node) => node.hasAttribute("data-reveal-hidden"))).toEqual([
      false,
      true
    ]);
    expect(emphasis.map((node) => node.hasAttribute("data-emphasis-active"))).toEqual([
      true,
      false
    ]);

    applyStepState(root, 2);
    expect(reveals.map((node) => node.hasAttribute("data-reveal-hidden"))).toEqual([
      false,
      false
    ]);
    expect(emphasis.map((node) => node.hasAttribute("data-emphasis-active"))).toEqual([
      true,
      true
    ]);
  }

  const css = createStepStateStyle(document).textContent ?? "";
  expect(css).toContain("[data-reveal-hidden]{visibility:hidden}");
  expect(css).toContain(
    ".code-line[data-emphasis-active],.code-range[data-emphasis-active]{"
  );
  expect(css).toContain(".code-line[data-emphasis-active]{box-shadow:");
  expect(css).toContain(".code-range[data-emphasis-active]{border-radius:");
  expect(css).toContain("pre:has([data-emphasis-active])");
  expect(css).toContain(":not(:has([data-emphasis-active]))");
  expect(css).not.toMatch(/(?:^|})\[data-emphasis-active\]\{/);
});
