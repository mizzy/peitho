import { isComposingKey } from "./keyboard";
import { readErrorResponse } from "./previewHttp";

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
  cancel(): void;
  setFrame(frame: PreviewSourceEditFrame): void;
  destroy(): void;
};

const INVALID_RESPONSE_MESSAGE = "slide source save returned an invalid response";

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

  let closed = false;
  let commitPromise: Promise<PreviewSourceEditCommitResult> | null = null;

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
      response = await options.fetcher("/slide-source", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ key: options.key, old: options.body, new: newBody }),
        keepalive: false
      });
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

    textarea.readOnly = true;
    const request = post(newBody);
    commitPromise = request;
    const clearCommit = (): void => {
      if (commitPromise === request) commitPromise = null;
    };
    void request.then(clearCommit, clearCommit);
    return request;
  };

  return {
    textarea,
    commit,
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
