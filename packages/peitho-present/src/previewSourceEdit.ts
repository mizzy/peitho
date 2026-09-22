import { isComposingKey } from "./keyboard";
import { postJson, readErrorResponse } from "./previewHttp";

export type PreviewSourceEditFrame = {
  left: number;
  top: number;
  width: number;
  height: number;
};

export type PreviewSourceEditCommitResult =
  | { status: "saved"; previousKey: string; key: string; body: string }
  | { status: "unchanged"; key: string; body: string }
  | { status: "closed" }
  | { status: "failed"; message: string };

export type PreviewSourceEdit = {
  readonly textarea: HTMLTextAreaElement;
  commit(): Promise<PreviewSourceEditCommitResult>;
  saveForPageHide(): void;
  cancel(): void;
  setFrame(frame: PreviewSourceEditFrame): void;
  destroy(): void;
};

const INVALID_RESPONSE_MESSAGE = "slide source save returned an invalid response";

/** Editor chrome, kept in step with the notes panel's dark palette. */
const SOURCE_EDITOR_BACKGROUND = "#15181e";
const SOURCE_EDITOR_COLOR = "#e5e7eb";
const SOURCE_EDITOR_CARET = "#38bdf8";
const SOURCE_EDITOR_FONT_SIZE = "18px";
const SOURCE_EDITOR_LINE_HEIGHT = "1.6";
const SOURCE_EDITOR_PADDING = "24px";

export function openPreviewSourceEdit(options: {
  document: Document;
  fetcher: typeof fetch;
  tile: HTMLElement;
  key: string;
  body: string;
  onCommitRequest(): void;
  onCancelRequest(): void;
}): PreviewSourceEdit {
  const textarea = options.document.createElement("textarea");
  textarea.dataset.peithoPreview = "source";
  textarea.setAttribute("aria-label", "Slide Markdown source");
  textarea.spellcheck = false;
  textarea.wrap = "off";
  textarea.value = options.body;
  textarea.style.position = "absolute";
  textarea.style.boxSizing = "border-box";
  textarea.style.margin = "0";
  textarea.style.resize = "none";
  textarea.style.fontFamily =
    "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace";
  textarea.style.tabSize = "2";
  textarea.style.whiteSpace = "pre";
  textarea.style.overflow = "auto";
  // The browser default is small black-on-white, which is unreadable against the
  // shell's dark ground. Deck CSS cannot reach this element, so it is styled here.
  textarea.style.background = SOURCE_EDITOR_BACKGROUND;
  textarea.style.color = SOURCE_EDITOR_COLOR;
  textarea.style.caretColor = SOURCE_EDITOR_CARET;
  textarea.style.fontSize = SOURCE_EDITOR_FONT_SIZE;
  textarea.style.lineHeight = SOURCE_EDITOR_LINE_HEIGHT;
  textarea.style.padding = SOURCE_EDITOR_PADDING;
  textarea.style.border = "none";

  let closed = false;
  let commitPromise: Promise<PreviewSourceEditCommitResult> | null = null;
  let committingBody: string | null = null;
  let sentForPageHide = false;

  const onKeyDown = (event: KeyboardEvent): void => {
    if (isComposingKey(event)) return;
    if (event.key === "Enter") {
      event.stopPropagation();
      if (!event.metaKey && !event.ctrlKey) return;
      event.preventDefault();
      if (commitPromise === null) options.onCommitRequest();
      return;
    }
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    if (commitPromise === null) options.onCancelRequest();
  };
  const onBlur = (): void => {
    if (commitPromise === null) options.onCommitRequest();
  };
  textarea.addEventListener("keydown", onKeyDown);
  textarea.addEventListener("blur", onBlur);

  options.tile.appendChild(textarea);
  textarea.focus({ preventScroll: true });
  textarea.setSelectionRange(0, 0);

  const close = (): void => {
    if (closed) return;
    closed = true;
    textarea.removeEventListener("keydown", onKeyDown);
    textarea.removeEventListener("blur", onBlur);
    textarea.remove();
  };

  const fail = (message: string): PreviewSourceEditCommitResult => {
    textarea.readOnly = false;
    if (!closed) textarea.focus({ preventScroll: true });
    return { status: "failed", message };
  };

  const post = async (newBody: string): Promise<PreviewSourceEditCommitResult> => {
    let response: Response;
    try {
      response = await postJson(
        options.fetcher,
        "/slide-source",
        { key: options.key, old: options.body, new: newBody },
        false
      );
    } catch (error) {
      return fail(`failed to save slide source: ${String(error)}`);
    }

    if (!response.ok) {
      try {
        return fail(await readErrorResponse(response, "slide source save"));
      } catch (error) {
        return fail(`failed to save slide source: ${String(error)}`);
      }
    }

    let value: unknown;
    try {
      value = await response.json();
    } catch {
      return fail(INVALID_RESPONSE_MESSAGE);
    }
    if (
      typeof value !== "object" ||
      value === null ||
      typeof (value as { key?: unknown }).key !== "string" ||
      typeof (value as { body?: unknown }).body !== "string"
    ) {
      return fail(INVALID_RESPONSE_MESSAGE);
    }

    const saved = value as { key: string; body: string };
    close();
    return {
      status: "saved",
      previousKey: options.key,
      key: saved.key,
      body: saved.body
    };
  };

  const commit = (): Promise<PreviewSourceEditCommitResult> => {
    if (closed) return Promise.resolve({ status: "closed" });
    if (commitPromise !== null) return commitPromise;
    const newBody = textarea.value;
    if (newBody === options.body) {
      close();
      return Promise.resolve({ status: "unchanged", key: options.key, body: options.body });
    }

    committingBody = newBody;
    textarea.readOnly = true;
    const request = post(newBody);
    commitPromise = request;
    const clearCommit = (): void => {
      if (commitPromise !== request) return;
      commitPromise = null;
      committingBody = null;
    };
    void request.then(clearCommit, clearCommit);
    return request;
  };

  const saveForPageHide = (): void => {
    if (closed || sentForPageHide) return;
    // Repeating the captured commit is unload insurance: its fetch is not keepalive, and the drift check refuses a same-old duplicate.
    const newBody = committingBody ?? textarea.value;
    if (newBody === options.body) return;
    sentForPageHide = true;
    try {
      const request = postJson(
        options.fetcher,
        "/slide-source",
        { key: options.key, old: options.body, new: newBody },
        true
      );
      void request.then(
        (response) => {
          if (!response.ok) sentForPageHide = false;
        },
        () => {
          sentForPageHide = false;
        }
      );
    } catch {
      sentForPageHide = false;
    }
  };

  return {
    textarea,
    commit,
    saveForPageHide,
    cancel: close,
    setFrame(frame): void {
      textarea.style.left = `${frame.left}px`;
      textarea.style.top = `${frame.top}px`;
      textarea.style.width = `${frame.width}px`;
      textarea.style.height = `${frame.height}px`;
    },
    destroy: close
  };
}
