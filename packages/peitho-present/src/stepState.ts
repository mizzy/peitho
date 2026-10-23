const REVEAL_HIDDEN_CSS = "[data-reveal-hidden]{visibility:hidden}";

/**
 * Stepped code line emphasis defaults shared by every step-aware renderer.
 *
 * Static emphasis defaults ship in the rendered deck CSS so every output and
 * every slot name receives them. This module owns only stepped emphasis
 * (`[data-emphasis-active]`, toggled per step). Whole lines keep the inset
 * marker bar; `.code-range` spans receive a tight rounded box. It retains the
 * `.code-line` layout rule because stepped-only decks do not receive the
 * static CSS block.
 *
 * Appearance stays themeable through the custom properties; a deck overriding
 * them in its own CSS wins, because these rules only supply the fallbacks.
 */
const EMPHASIS_ACTIVE_CSS = [
  ".code-line{display:inline-block;width:100%}",
  ".code-line[data-emphasis-active],.code-range[data-emphasis-active]{",
  "background:var(--peitho-emphasis-background,rgba(217,163,0,0.18));",
  "}",
  ".code-line[data-emphasis-active]{",
  "box-shadow:inset 3px 0 0 var(--peitho-emphasis-marker,#d9a300)",
  "}",
  ".code-range[data-emphasis-active]{border-radius:.15em}",
  "pre:has([data-emphasis-active]) .code-line:not([data-emphasis-active]):not(:has([data-emphasis-active])){",
  "opacity:var(--peitho-emphasis-dim,0.45)",
  "}"
].join("");

export function createStepStateStyle(doc: Document): HTMLStyleElement {
  const style = doc.createElement("style");
  style.textContent = REVEAL_HIDDEN_CSS + EMPHASIS_ACTIVE_CSS;
  return style;
}

/**
 * Move code line emphasis to every marker listing the current step.
 *
 * Unlike reveal, emphasis is not cumulative: it points at one group at a
 * time and moves, so each whitespace-separated step token is compared for
 * equality rather than "step or earlier". Stepping past the last group
 * leaves the block unemphasized, which is the correct final state — the
 * pointer is gone once the speaker has moved on.
 */
function applyEmphasisState(root: ParentNode, step: number): void {
  for (const marker of root.querySelectorAll<HTMLElement>("[data-emphasis-step]")) {
    const steps = (marker.dataset.emphasisStep ?? "")
      .trim()
      .split(/\s+/)
      .filter(Boolean)
      .map(Number);
    marker.toggleAttribute("data-emphasis-active", steps.includes(step));
  }
}

export function applyStepState(root: ParentNode, step: number): void {
  for (const marker of root.querySelectorAll<HTMLElement>("[data-reveal-step]")) {
    const revealStep = Number(marker.dataset.revealStep);
    marker.toggleAttribute(
      "data-reveal-hidden",
      Number.isFinite(revealStep) && revealStep > step
    );
  }

  applyEmphasisState(root, step);
}
