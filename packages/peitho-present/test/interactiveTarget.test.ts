import { afterEach, describe, expect, it } from "vitest";
import { keyBelongsToTarget, pointerBelongsToTarget } from "../src/interactiveTarget";

type ShadowFixture = {
  host: HTMLElement;
  origin: HTMLElement;
};

function shadowFixture(tagName: keyof HTMLElementTagNameMap): ShadowFixture {
  const host = document.createElement("div");
  const origin = document.createElement(tagName);
  host.attachShadow({ mode: "open" }).appendChild(origin);
  document.body.appendChild(host);
  return { host, origin };
}

function nestedShadowFixture(
  tagName: keyof HTMLElementTagNameMap,
  attributes: Record<string, string> = {}
): ShadowFixture {
  const host = document.createElement("div");
  const parent = document.createElement(tagName);
  for (const [name, value] of Object.entries(attributes)) parent.setAttribute(name, value);
  const origin = document.createElement("span");
  parent.appendChild(origin);
  host.attachShadow({ mode: "open" }).appendChild(parent);
  document.body.appendChild(host);
  return { host, origin };
}

function slideInputFixture(type?: string): { host: HTMLElement; input: HTMLInputElement } {
  const host = document.createElement("section");
  host.dataset.slideKey = "intro";
  const input = document.createElement("input");
  if (type !== undefined) input.setAttribute("type", type);
  host.attachShadow({ mode: "open" }).appendChild(input);
  document.body.appendChild(host);
  return { host, input };
}

function keyResult(origin: Element, key: string, init: KeyboardEventInit = {}): boolean {
  let result: boolean | undefined;
  const listener = (event: KeyboardEvent): void => {
    result = keyBelongsToTarget(event);
  };
  window.addEventListener("keydown", listener, { once: true });
  origin.dispatchEvent(
    new KeyboardEvent("keydown", {
      key,
      bubbles: true,
      composed: true,
      cancelable: true,
      ...init
    })
  );
  if (result === undefined) throw new Error("Keyboard event did not reach window");
  return result;
}

function clickResult(origin: Element): boolean {
  let result: boolean | undefined;
  const listener = (event: MouseEvent): void => {
    result = pointerBelongsToTarget(event);
  };
  window.addEventListener("click", listener, { once: true });
  origin.dispatchEvent(
    new MouseEvent("click", {
      bubbles: true,
      composed: true,
      cancelable: true
    })
  );
  if (result === undefined) throw new Error("Click event did not reach window");
  return result;
}

afterEach(() => document.body.replaceChildren());

describe("keyBelongsToTarget", () => {
  const editableFixtures: Array<[string, () => ShadowFixture]> = [
    ["input", () => shadowFixture("input")],
    ["textarea", () => shadowFixture("textarea")],
    ["select", () => shadowFixture("select")],
    ["contenteditable", () => nestedShadowFixture("div", { contenteditable: "true" })]
  ];
  const editableKeys = [
    "a",
    " ",
    "Escape",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Enter"
  ];

  it.each(editableFixtures)("lets %s own editing keys", (_name, createFixture) => {
    const { origin } = createFixture();

    for (const key of editableKeys) expect(keyResult(origin, key), key).toBe(true);
  });

  it.each(editableFixtures)(
    "lets unshifted page keys escape %s but keeps shifted page keys",
    (_name, createFixture) => {
      const { origin } = createFixture();

      for (const key of ["PageUp", "PageDown"]) {
        expect(keyResult(origin, key), key).toBe(false);
        expect(keyResult(origin, key, { shiftKey: true }), `Shift+${key}`).toBe(true);
      }
    }
  );

  it("does not treat contenteditable=false as editable", () => {
    const { origin } = nestedShadowFixture("div", { contenteditable: "false" });

    for (const key of [...editableKeys, "PageUp", "PageDown"]) {
      expect(keyResult(origin, key), key).toBe(false);
    }
  });

  it("lets a link in a slide own Enter but not Space", () => {
    const { host, origin } = nestedShadowFixture("a", { href: "#target" });
    host.dataset.slideKey = "intro";

    expect(keyResult(origin, "Enter")).toBe(true);
    expect(keyResult(origin, " ")).toBe(false);
    expect(keyResult(origin, "ArrowRight")).toBe(false);
    expect(keyResult(origin, "x")).toBe(false);
  });

  it.each(["button", "summary"] as const)(
    "lets %s in a slide own Enter and Space but not other keys",
    (tagName) => {
      const { host, origin } = nestedShadowFixture(tagName);
      host.dataset.slideKey = "intro";

      expect(keyResult(origin, "Enter")).toBe(true);
      expect(keyResult(origin, " ")).toBe(true);
      expect(keyResult(origin, "ArrowRight")).toBe(false);
      expect(keyResult(origin, "x")).toBe(false);
    }
  );

  it("lets a button in a nested shadow root inside a slide own Enter and Space", () => {
    const slideHost = document.createElement("section");
    slideHost.dataset.slideKey = "intro";
    const innerHost = document.createElement("div");
    slideHost.attachShadow({ mode: "open" }).appendChild(innerHost);
    const button = document.createElement("button");
    innerHost.attachShadow({ mode: "open" }).appendChild(button);
    document.body.appendChild(slideHost);

    expect(keyResult(button, "Enter")).toBe(true);
    expect(keyResult(button, " ")).toBe(true);
  });

  it("lets a button in a light-DOM data-slide-key section own Enter and Space", () => {
    const section = document.createElement("section");
    section.dataset.slideKey = "intro";
    const button = document.createElement("button");
    section.appendChild(button);
    document.body.appendChild(section);

    expect(keyResult(button, "Enter")).toBe(true);
    expect(keyResult(button, " ")).toBe(true);
  });

  it("finds a light-DOM data-slide-key ancestor outside a nested shadow root", () => {
    const section = document.createElement("section");
    section.dataset.slideKey = "intro";
    const innerHost = document.createElement("div");
    const button = document.createElement("button");
    innerHost.attachShadow({ mode: "open" }).appendChild(button);
    section.appendChild(innerHost);
    document.body.appendChild(section);

    expect(keyResult(button, "Enter")).toBe(true);
    expect(keyResult(button, " ")).toBe(true);
  });

  it("lets a slide checkbox own Space but not shell shortcut keys", () => {
    const { input } = slideInputFixture("checkbox");

    expect(keyResult(input, " ")).toBe(true);
    expect(keyResult(input, "ArrowRight")).toBe(false);
    expect(keyResult(input, "f")).toBe(false);
    expect(keyResult(input, "Escape")).toBe(false);
  });

  it("lets a slide range input own arrows, Home, End, PageUp, and PageDown", () => {
    const { input } = slideInputFixture("range");

    for (const key of [
      "ArrowLeft",
      "ArrowRight",
      "ArrowUp",
      "ArrowDown",
      "Home",
      "End",
      "PageUp",
      "PageDown"
    ]) {
      expect(keyResult(input, key), key).toBe(true);
    }
    expect(keyResult(input, "f")).toBe(false);
  });

  it("lets a slide radio input own arrow keys", () => {
    const { input } = slideInputFixture("radio");

    expect(keyResult(input, "ArrowDown")).toBe(true);
  });

  it.each([undefined, "text", "number", "date"])(
    "treats input type %s as text entry",
    (type) => {
      const { input } = slideInputFixture(type);

      expect(keyResult(input, "x")).toBe(true);
      expect(keyResult(input, " ")).toBe(true);
    }
  );

  it("uses the normalized input type for unknown values", () => {
    const { input } = slideInputFixture("foo");

    expect(input.type).toBe("text");
    expect(keyResult(input, "x")).toBe(true);
  });

  it("does not let a light-DOM button own Enter or Space", () => {
    const button = document.createElement("button");
    document.body.appendChild(button);

    expect(keyResult(button, "Enter")).toBe(false);
    expect(keyResult(button, " ")).toBe(false);
  });

  it("does not let a button in an unmarked shadow root own Enter or Space", () => {
    const { origin } = nestedShadowFixture("button");

    expect(keyResult(origin, "Enter")).toBe(false);
    expect(keyResult(origin, " ")).toBe(false);
  });

  it("does not let a plain div own keys", () => {
    const { origin } = shadowFixture("div");

    for (const key of [...editableKeys, "PageUp", "PageDown"]) {
      expect(keyResult(origin, key), key).toBe(false);
    }
  });
});

describe("pointerBelongsToTarget", () => {
  const interactiveFixtures: Array<[string, () => ShadowFixture]> = [
    ["a", () => nestedShadowFixture("a")],
    ["button", () => nestedShadowFixture("button")],
    ["summary", () => nestedShadowFixture("summary")],
    ["input", () => shadowFixture("input")],
    ["textarea", () => shadowFixture("textarea")],
    ["select", () => shadowFixture("select")],
    ["label", () => nestedShadowFixture("label")],
    ["contenteditable", () => nestedShadowFixture("div", { contenteditable: "true" })]
  ];

  it.each(interactiveFixtures)("lets %s own clicks", (_name, createFixture) => {
    const { origin } = createFixture();

    expect(clickResult(origin)).toBe(true);
  });

  it("does not let a plain div own clicks", () => {
    const { origin } = shadowFixture("div");

    expect(clickResult(origin)).toBe(false);
  });
});
