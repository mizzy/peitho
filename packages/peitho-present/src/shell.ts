import type { Manifest } from "../../../bindings/Manifest";
import type { ManifestSlide } from "../../../bindings/ManifestSlide";

export type NavigateTarget =
  | "next"
  | "prev"
  | "first"
  | "last"
  | { key: string }
  | { index: number };

export type NavigateDetail = { to: NavigateTarget };

export type SlideChangeDetail = {
  key: string;
  index: number;
  total: number;
  previousIndex: number | null;
};

export type PresentShell = {
  manifest: Manifest | null;
  currentIndex: number;
  navigate(to: NavigateTarget): void;
  destroy(): void;
};

export type ShellOptions = {
  root: HTMLElement;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  console?: Pick<Console, "error">;
};

export type SlideView = {
  meta: ManifestSlide;
  host: HTMLElement;
};

export async function mountPresentShell(options: ShellOptions): Promise<PresentShell> {
  const shell = new PresentShellController(options);
  await shell.load();
  return shell;
}

class PresentShellController implements PresentShell {
  manifest: Manifest | null = null;
  currentIndex = -1;
  private readonly slides: SlideView[] = [];
  private readonly root: HTMLElement;
  private readonly fetcher: typeof fetch;
  private readonly win: Window;
  private readonly doc: Document;
  private readonly log: Pick<Console, "error">;
  private readonly onNavigate = (event: Event): void => {
    const detail = (event as CustomEvent<NavigateDetail>).detail;
    if (!detail || !("to" in detail)) {
      this.log.error("Invalid peitho:navigate event");
      return;
    }
    this.navigate(detail.to);
  };

  constructor(options: ShellOptions) {
    this.root = options.root;
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.log = options.console ?? console;
    this.win.addEventListener("peitho:navigate", this.onNavigate);
  }

  async load(): Promise<void> {
    try {
      const manifest = await this.fetchJson<Manifest>("manifest.json");
      const pending: SlideView[] = [];
      for (const slide of manifest.slides) {
        const html = await this.fetchText(slide.src);
        const host = this.createSlideHost(slide, html);
        pending.push({ meta: slide, host });
      }
      this.manifest = manifest;
      for (const view of pending) {
        this.root.appendChild(view.host);
        this.slides.push(view);
      }
      this.show(0);
    } catch (error) {
      this.root.replaceChildren();
      this.root.textContent = error instanceof Error ? error.message : String(error);
    }
  }

  navigate(to: NavigateTarget): void {
    const index = this.resolveTarget(to);
    if (index === null) return;
    this.show(index);
  }

  destroy(): void {
    this.win.removeEventListener("peitho:navigate", this.onNavigate);
  }

  private async fetchJson<T>(url: string): Promise<T> {
    const response = await this.fetchOk(url);
    return response.json() as Promise<T>;
  }

  private async fetchText(url: string): Promise<string> {
    const response = await this.fetchOk(url);
    return response.text();
  }

  private async fetchOk(url: string): Promise<Response> {
    const response = await this.fetcher(url);
    if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
    return response;
  }

  private createSlideHost(slide: ManifestSlide, html: string): HTMLElement {
    const host = this.doc.createElement("section");
    host.classList.add("peitho-slide");
    host.dataset.slideKey = slide.key;
    host.dataset.slideIndex = String(slide.index);
    host.attachShadow({ mode: "open" }).innerHTML = html;
    return host;
  }

  private resolveTarget(to: NavigateTarget): number | null {
    if (to === "first") return 0;
    if (to === "last") return this.slides.length - 1;
    if (to === "next") return Math.min(this.currentIndex + 1, this.slides.length - 1);
    if (to === "prev") return Math.max(this.currentIndex - 1, 0);
    if ("index" in to) {
      if (to.index < 0 || to.index >= this.slides.length) {
        this.log.error(`Unknown slide index: ${to.index}`);
        return null;
      }
      return to.index;
    }
    const index = this.slides.findIndex((slide) => slide.meta.key === to.key);
    if (index < 0) {
      this.log.error(`Unknown slide key: ${to.key}`);
      return null;
    }
    return index;
  }

  private show(index: number): void {
    if (index < 0 || index >= this.slides.length) {
      this.log.error(`Unknown slide target: ${index}`);
      return;
    }
    if (index === this.currentIndex) return;

    this.slides.forEach((slide, slideIndex) => {
      slide.host.hidden = slideIndex !== index;
    });
    const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
    this.currentIndex = index;
    const slide = this.slides[index];
    this.win.dispatchEvent(
      new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
        detail: {
          key: slide.meta.key,
          index: slide.meta.index,
          total: this.slides.length,
          previousIndex
        }
      })
    );
  }
}
