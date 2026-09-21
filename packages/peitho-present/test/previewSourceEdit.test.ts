import { afterEach, expect, it, vi } from "vitest";
import {
  openPreviewSourceEdit,
  type PreviewSourceEdit,
  type PreviewSourceEditCommitResult
} from "../src/previewSourceEdit";

function jsonResponse(status: number, value: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => value,
    text: async () => JSON.stringify(value)
  } as unknown as Response;
}

function textResponse(status: number, value: string): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => {
      throw new SyntaxError("invalid JSON");
    },
    text: async () => value
  } as unknown as Response;
}

function deferredResponse(): {
  promise: Promise<Response>;
  resolve(response: Response): void;
  reject(error: unknown): void;
} {
  let resolve!: (response: Response) => void;
  let reject!: (error: unknown) => void;
  return {
    promise: new Promise<Response>((settle, fail) => {
      resolve = settle;
      reject = fail;
    }),
    resolve(response): void {
      resolve(response);
    },
    reject(error): void {
      reject(error);
    }
  };
}

type OpenFixture = {
  edit: PreviewSourceEdit;
  tile: HTMLElement;
  host: HTMLElement;
  fetchMock: ReturnType<typeof vi.fn>;
  onCommitRequest: ReturnType<typeof vi.fn>;
  onCancelRequest: ReturnType<typeof vi.fn>;
};

const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.restoreAllMocks();
});

function openForTest(options: {
  body?: string;
  key?: string;
  fetcher?: typeof fetch;
} = {}): OpenFixture {
  const tile = document.createElement("div");
  const host = document.createElement("div");
  host.attachShadow({ mode: "open" }).append(document.createElement("section"));
  tile.append(host);
  document.body.append(tile);

  const fetchMock = vi.fn(async () => {
    throw new Error("unexpected fetch");
  });
  const onCommitRequest = vi.fn();
  const onCancelRequest = vi.fn();
  const edit = openPreviewSourceEdit({
    document,
    fetcher: options.fetcher ?? (fetchMock as unknown as typeof fetch),
    tile,
    key: options.key ?? "old-key",
    body: options.body ?? "# Old",
    onCommitRequest,
    onCancelRequest
  });
  cleanups.push(() => {
    edit.destroy();
    tile.remove();
  });
  return { edit, tile, host, fetchMock, onCommitRequest, onCancelRequest };
}

function dispatchKey(
  target: HTMLElement,
  key: string,
  init: KeyboardEventInit = {}
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
    ...init
  });
  target.dispatchEvent(event);
  return event;
}

function focusAway(): void {
  const button = document.createElement("button");
  document.body.append(button);
  cleanups.push(() => button.remove());
  button.focus();
  expect(document.activeElement).toBe(button);
}

it("opens a light-DOM source textarea and applies the fitted stage frame", () => {
  const { edit, tile, host, onCancelRequest, onCommitRequest } = openForTest({
    body: "# Supplied body"
  });
  const { textarea } = edit;

  expect(textarea.parentElement).toBe(tile);
  expect(host.shadowRoot?.contains(textarea)).toBe(false);
  expect(textarea.dataset.peithoPreview).toBe("source");
  expect(textarea.style.fontFamily).toContain("monospace");
  expect(textarea.style.tabSize).toBe("2");
  expect(textarea.style.whiteSpace).toBe("pre");
  expect(textarea.wrap).toBe("off");
  expect(textarea.spellcheck).toBe(false);
  expect(textarea.getAttribute("aria-label")).toBe("Slide Markdown source");
  expect(textarea.value).toBe("# Supplied body");
  expect(document.activeElement).toBe(textarea);
  expect(textarea.selectionStart).toBe(0);
  expect(textarea.selectionEnd).toBe(0);
  expect(host.hidden).toBe(false);

  edit.setFrame({ left: 12.5, top: 24, width: 640, height: 360 });
  expect(textarea.style.left).toBe("12.5px");
  expect(textarea.style.top).toBe("24px");
  expect(textarea.style.width).toBe("640px");
  expect(textarea.style.height).toBe("360px");
  expect(textarea.style.transform).toBe("");

  host.hidden = true;
  edit.cancel();
  expect(textarea.isConnected).toBe(false);
  expect(host.hidden).toBe(true);
  dispatchKey(textarea, "Escape");
  textarea.dispatchEvent(new FocusEvent("blur"));
  expect(onCancelRequest).not.toHaveBeenCalled();
  expect(onCommitRequest).not.toHaveBeenCalled();
});

it("keeps plain Enter local, lets PageDown propagate, and emits commit and cancel requests", () => {
  const { edit, onCommitRequest, onCancelRequest } = openForTest();
  const windowKeys: string[] = [];
  const onWindowKeyDown = (event: KeyboardEvent): void => {
    windowKeys.push(event.key);
  };
  window.addEventListener("keydown", onWindowKeyDown);
  cleanups.push(() => window.removeEventListener("keydown", onWindowKeyDown));

  const enter = dispatchKey(edit.textarea, "Enter");
  expect(enter.defaultPrevented).toBe(false);
  expect(onCommitRequest).not.toHaveBeenCalled();
  expect(windowKeys).toEqual([]);

  dispatchKey(edit.textarea, "PageDown");
  expect(windowKeys).toEqual(["PageDown"]);

  const metaEnter = dispatchKey(edit.textarea, "Enter", { metaKey: true });
  const ctrlEnter = dispatchKey(edit.textarea, "Enter", { ctrlKey: true });
  expect(metaEnter.defaultPrevented).toBe(true);
  expect(ctrlEnter.defaultPrevented).toBe(true);
  expect(onCommitRequest).toHaveBeenCalledTimes(2);
  expect(windowKeys).toEqual(["PageDown"]);

  const escape = dispatchKey(edit.textarea, "Escape");
  expect(escape.defaultPrevented).toBe(true);
  expect(onCancelRequest).toHaveBeenCalledTimes(1);
  expect(windowKeys).toEqual(["PageDown"]);

  edit.textarea.dispatchEvent(new FocusEvent("blur"));
  expect(onCommitRequest).toHaveBeenCalledTimes(3);
});

it("ignores composing Enter and Escape, including keyCode 229", () => {
  const { edit, onCommitRequest, onCancelRequest } = openForTest();
  const composingEnter = dispatchKey(edit.textarea, "Enter", { isComposing: true });
  const composingEscape = dispatchKey(edit.textarea, "Escape", { isComposing: true });
  const legacyComposition = new KeyboardEvent("keydown", {
    key: "Escape",
    bubbles: true,
    cancelable: true
  });
  Object.defineProperty(legacyComposition, "keyCode", { value: 229 });
  edit.textarea.dispatchEvent(legacyComposition);

  expect(composingEnter.defaultPrevented).toBe(false);
  expect(composingEscape.defaultPrevented).toBe(false);
  expect(legacyComposition.defaultPrevented).toBe(false);
  expect(onCommitRequest).not.toHaveBeenCalled();
  expect(onCancelRequest).not.toHaveBeenCalled();
});

it("skips the POST only when the textarea value is byte-identical", async () => {
  const { edit, fetchMock, onCommitRequest, onCancelRequest } = openForTest({
    body: "# Old"
  });
  const { textarea } = edit;

  await expect(edit.commit()).resolves.toEqual({
    status: "unchanged",
    key: "old-key",
    body: "# Old"
  });
  expect(fetchMock).not.toHaveBeenCalled();
  expect(textarea.isConnected).toBe(false);
  await expect(edit.commit()).resolves.toEqual({ status: "closed" });

  dispatchKey(textarea, "Enter", { metaKey: true });
  dispatchKey(textarea, "Escape");
  textarea.dispatchEvent(new FocusEvent("blur"));
  expect(onCommitRequest).not.toHaveBeenCalled();
  expect(onCancelRequest).not.toHaveBeenCalled();
});

it.each([
  ["a trailing ideographic-space line", "# Old\n\u3000"],
  ["a leading non-breaking-space line", "\u00a0\n# Old"],
  ["a trailing newline", "# Old\n"]
])("posts %s instead of silently dropping it", async (_description, draft) => {
  const fetchMock = vi.fn(async () => jsonResponse(200, { key: "old-key", body: draft }));
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = draft;

  await expect(edit.commit()).resolves.toEqual({
    status: "saved",
    previousKey: "old-key",
    key: "old-key",
    body: draft
  });
  expect(fetchMock).toHaveBeenCalledTimes(1);
  expect(fetchMock).toHaveBeenCalledWith(
    "/slide-source",
    expect.objectContaining({
      body: JSON.stringify({ key: "old-key", old: "# Old", new: draft })
    })
  );
});

it("does not commit a discarded draft through a closed handle", async () => {
  const { edit, fetchMock } = openForTest();
  edit.textarea.value = "# Discarded draft";

  edit.cancel();

  await expect(edit.commit()).resolves.toEqual({ status: "closed" });
  expect(fetchMock).not.toHaveBeenCalled();
});

it("posts the raw draft once and returns the server key and body verbatim", async () => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(() => pending.promise);
  const { edit, tile, onCommitRequest, onCancelRequest } = openForTest({
    fetcher: fetchMock as unknown as typeof fetch
  });
  const parentKeys: string[] = [];
  const onParentKeyDown = (event: KeyboardEvent): void => {
    parentKeys.push(event.key);
  };
  tile.addEventListener("keydown", onParentKeyDown);
  cleanups.push(() => tile.removeEventListener("keydown", onParentKeyDown));
  const draft = "\r\n# Typed\r\n";
  edit.textarea.value = draft;
  const submitted = edit.textarea.value;
  expect(submitted).toBe("\n# Typed\n");

  dispatchKey(edit.textarea, "PageDown");
  expect(parentKeys).toEqual(["PageDown"]);
  parentKeys.length = 0;

  const first = edit.commit();
  const second = edit.commit();
  expect(second).toBe(first);
  expect(edit.textarea.readOnly).toBe(true);
  expect(fetchMock).toHaveBeenCalledTimes(1);
  expect(fetchMock).toHaveBeenCalledWith("/slide-source", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ key: "old-key", old: "# Old", new: submitted }),
    keepalive: false
  });

  edit.textarea.dispatchEvent(new FocusEvent("blur"));
  const metaEnter = dispatchKey(edit.textarea, "Enter", { metaKey: true });
  const ctrlEnter = dispatchKey(edit.textarea, "Enter", { ctrlKey: true });
  const escape = dispatchKey(edit.textarea, "Escape");
  dispatchKey(edit.textarea, "PageDown");
  expect(metaEnter.defaultPrevented).toBe(true);
  expect(ctrlEnter.defaultPrevented).toBe(true);
  expect(escape.defaultPrevented).toBe(true);
  expect(parentKeys).toEqual(["PageDown"]);
  expect(onCommitRequest).not.toHaveBeenCalled();
  expect(onCancelRequest).not.toHaveBeenCalled();

  pending.resolve(jsonResponse(200, { key: "renamed", body: "# Server canonical" }));
  await expect(first).resolves.toEqual({
    status: "saved",
    previousKey: "old-key",
    key: "renamed",
    body: "# Server canonical"
  });
  expect(edit.textarea.value).toBe(submitted);
  expect(edit.textarea.isConnected).toBe(false);
  await expect(edit.commit()).resolves.toEqual({ status: "closed" });
  expect(fetchMock).toHaveBeenCalledTimes(1);
  dispatchKey(edit.textarea, "Enter", { metaKey: true });
  dispatchKey(edit.textarea, "Escape");
  edit.textarea.dispatchEvent(new FocusEvent("blur"));
  expect(onCommitRequest).not.toHaveBeenCalled();
  expect(onCancelRequest).not.toHaveBeenCalled();
});

it.each([
  [409, "the deck changed on disk; reload and retry"],
  [
    422,
    "slide body edit would change the deck's slide count\n  = help: remove the slide separator"
  ],
  [500, "failed to write deck.md"]
])("returns the exact JSON error for HTTP %i and keeps the draft", async (status, message) => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(() => pending.promise);
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = "# Draft";

  const commit = edit.commit();
  expect(edit.textarea.readOnly).toBe(true);
  focusAway();
  pending.resolve(jsonResponse(status, { error: message }));
  await expect(commit).resolves.toEqual({ status: "failed", message });
  expect(edit.textarea.value).toBe("# Draft");
  expect(edit.textarea.isConnected).toBe(true);
  expect(edit.textarea.readOnly).toBe(false);
  expect(document.activeElement).toBe(edit.textarea);
});

it("uses the HTTP fallback for a plain-text server error and refocuses", async () => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(() => pending.promise);
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = "# Draft";

  const commit = edit.commit();
  focusAway();
  pending.resolve(textResponse(400, "invalid slide source body\n"));
  await expect(commit).resolves.toEqual({
    status: "failed",
    message: "slide source save failed (HTTP 400)"
  });
  expect(edit.textarea.readOnly).toBe(false);
  expect(document.activeElement).toBe(edit.textarea);
});

it("turns a thrown fetch into the inline-edit-style failure, keeps the draft, and refocuses", async () => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(() => pending.promise);
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = "# Draft";

  const commit = edit.commit();
  focusAway();
  pending.reject(new Error("offline"));
  await expect(commit).resolves.toEqual({
    status: "failed",
    message: "failed to save slide source: Error: offline"
  });
  expect(edit.textarea.value).toBe("# Draft");
  expect(edit.textarea.readOnly).toBe(false);
  expect(document.activeElement).toBe(edit.textarea);
});

it.each([
  null,
  { body: "# Missing key" },
  { key: "renamed", body: 42 }
])("rejects an invalid success response without throwing", async (responseBody) => {
  const fetchMock = vi.fn(async () => jsonResponse(200, responseBody));
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = "# Draft";

  await expect(edit.commit()).resolves.toEqual({
    status: "failed",
    message: "slide source save returned an invalid response"
  });
  expect(edit.textarea.value).toBe("# Draft");
  expect(edit.textarea.isConnected).toBe(true);
  expect(edit.textarea.readOnly).toBe(false);
  expect(document.activeElement).toBe(edit.textarea);
});

it("treats invalid JSON in a success response as a clear failure", async () => {
  const fetchMock = vi.fn(async () => textResponse(200, "not JSON"));
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = "# Draft";

  const result: PreviewSourceEditCommitResult = await edit.commit();
  expect(result).toEqual({
    status: "failed",
    message: "slide source save returned an invalid response"
  });
  expect(edit.textarea.readOnly).toBe(false);
});

it("uses the HTTP fallback when error JSON does not contain a string error", async () => {
  const fetchMock = vi.fn(async () => jsonResponse(422, { error: 42 }));
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = "# Draft";

  await expect(edit.commit()).resolves.toEqual({
    status: "failed",
    message: "slide source save failed (HTTP 422)"
  });
});

it("handles a non-2xx response whose body read rejects", async () => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(() => pending.promise);
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = "# Draft";

  const commit = edit.commit();
  focusAway();
  pending.resolve({
    ok: false,
    status: 500,
    text: async () => {
      throw new Error("response body unavailable");
    }
  } as unknown as Response);

  await expect(commit).resolves.toEqual({
    status: "failed",
    message: "failed to save slide source: Error: response body unavailable"
  });
  expect(edit.textarea.readOnly).toBe(false);
  expect(document.activeElement).toBe(edit.textarea);
});

it("clears the in-flight promise after failure so a later commit can succeed", async () => {
  const fetchMock = vi
    .fn()
    .mockResolvedValueOnce(jsonResponse(422, { error: "fix the draft" }))
    .mockResolvedValueOnce(jsonResponse(200, { key: "new-key", body: "# Retried" }));
  const { edit } = openForTest({ fetcher: fetchMock as unknown as typeof fetch });
  edit.textarea.value = "# First attempt";

  await expect(edit.commit()).resolves.toEqual({
    status: "failed",
    message: "fix the draft"
  });

  edit.textarea.value = "# Retried";
  await expect(edit.commit()).resolves.toEqual({
    status: "saved",
    previousKey: "old-key",
    key: "new-key",
    body: "# Retried"
  });
  expect(fetchMock).toHaveBeenCalledTimes(2);
});

it("suppresses blur during a request and removes listeners on destroy", async () => {
  const pending = deferredResponse();
  const fetchMock = vi.fn(() => pending.promise);
  const { edit, onCommitRequest, onCancelRequest } = openForTest({
    fetcher: fetchMock as unknown as typeof fetch
  });
  const { textarea } = edit;
  textarea.value = "# Draft";
  const commit = edit.commit();

  textarea.dispatchEvent(new FocusEvent("blur"));
  expect(onCommitRequest).not.toHaveBeenCalled();

  edit.destroy();
  expect(textarea.isConnected).toBe(false);
  await expect(edit.commit()).resolves.toEqual({ status: "closed" });
  expect(fetchMock).toHaveBeenCalledTimes(1);
  dispatchKey(textarea, "Enter", { metaKey: true });
  dispatchKey(textarea, "Escape");
  textarea.dispatchEvent(new FocusEvent("blur"));
  expect(onCommitRequest).not.toHaveBeenCalled();
  expect(onCancelRequest).not.toHaveBeenCalled();

  pending.resolve(jsonResponse(200, { key: "new-key", body: "# Draft" }));
  await expect(commit).resolves.toEqual({
    status: "saved",
    previousKey: "old-key",
    key: "new-key",
    body: "# Draft"
  });
});
