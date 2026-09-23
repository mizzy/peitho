// The distribution viewer consumes these helpers through the IIFE bundle built from viewer.ts.

const CONTENTEDITABLE_SELECTOR = "[contenteditable]";
const INPUT_SELECTOR = "input";
const ENTER_ACTIVATABLE_SELECTOR = "a[href], button, summary";
const SPACE_ACTIVATABLE_SELECTOR = "button, summary";
const CLICK_INTERACTIVE_SELECTOR =
  "a, button, summary, input, textarea, select, label";
const TEXT_ENTRY_INPUT_TYPES =
  "text search email url tel password number date datetime-local month week time";
const SPACE_ACTIVATABLE_INPUT_TYPES =
  "checkbox radio button submit reset image color file";
const ENTER_ACTIVATABLE_INPUT_TYPES = "button submit reset image color file";
const ARROW_ACTIVATABLE_INPUT_TYPES = "radio range";
const ARROW_KEYS = "ArrowLeft ArrowRight ArrowUp ArrowDown";
const RANGE_KEYS = "Home End PageUp PageDown";

const textEntryInputTypes = new Set(TEXT_ENTRY_INPUT_TYPES.split(" "));
const spaceActivatableInputTypes = new Set(SPACE_ACTIVATABLE_INPUT_TYPES.split(" "));
const enterActivatableInputTypes = new Set(ENTER_ACTIVATABLE_INPUT_TYPES.split(" "));
const arrowActivatableInputTypes = new Set(ARROW_ACTIVATABLE_INPUT_TYPES.split(" "));
const arrowKeys = new Set(ARROW_KEYS.split(" "));
const rangeKeys = new Set(RANGE_KEYS.split(" "));

function isInsideSlide(origin: Node): boolean {
  let current = origin;
  while (true) {
    if (
      current instanceof Element &&
      current.closest("[data-slide-key]") !== null
    ) {
      return true;
    }
    const root = current.getRootNode();
    if (!(root instanceof ShadowRoot)) return false;
    if (root.host.hasAttribute("data-slide-key")) return true;
    current = root.host;
  }
}

export function isEditableTarget(event: Event): boolean {
  const target = event.composedPath()[0];
  if (
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement
  ) {
    return true;
  }
  if (target instanceof HTMLInputElement) return textEntryInputTypes.has(target.type);
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const editable = target.closest<HTMLElement>(CONTENTEDITABLE_SELECTOR);
  return editable !== null && editable.getAttribute("contenteditable") !== "false";
}

export function keyBelongsToTarget(event: KeyboardEvent): boolean {
  if (
    isEditableTarget(event) &&
    (event.shiftKey || (event.key !== "PageUp" && event.key !== "PageDown"))
  ) {
    return true;
  }
  const origin = event.composedPath()[0];
  if (!(origin instanceof Element) || !isInsideSlide(origin)) return false;
  if (
    event.key === "Enter" &&
    origin.closest(ENTER_ACTIVATABLE_SELECTOR) !== null
  ) {
    return true;
  }
  if (event.key === " " && origin.closest(SPACE_ACTIVATABLE_SELECTOR) !== null) {
    return true;
  }
  const input = origin.closest<HTMLInputElement>(INPUT_SELECTOR);
  if (input === null) return false;
  if (event.key === " ") return spaceActivatableInputTypes.has(input.type);
  if (event.key === "Enter") return enterActivatableInputTypes.has(input.type);
  if (arrowKeys.has(event.key)) return arrowActivatableInputTypes.has(input.type);
  return input.type === "range" && rangeKeys.has(event.key);
}

export function pointerBelongsToTarget(event: Event): boolean {
  const origin = event.composedPath()[0];
  if (origin instanceof Element && origin.closest(CLICK_INTERACTIVE_SELECTOR) !== null) {
    return true;
  }
  return isEditableTarget(event);
}
