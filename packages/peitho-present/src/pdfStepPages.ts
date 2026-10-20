import { applyStepState } from "./stepState";

export interface PdfStepPageSlide {
  readonly key: string;
  readonly revealSteps: number;
}

function validatedRevealStepCount(slide: PdfStepPageSlide): number {
  if (!Number.isSafeInteger(slide.revealSteps) || slide.revealSteps < 0) {
    throw new Error(
      `Slide ${JSON.stringify(slide.key)} has invalid revealSteps ${String(slide.revealSteps)}; expected a non-negative safe integer`
    );
  }
  return slide.revealSteps;
}

function copyCanvasBitmaps(
  source: ParentNode,
  copy: ParentNode,
  key: string
): void {
  const sourceCanvases = Array.from(
    source.querySelectorAll<HTMLCanvasElement>("canvas")
  );
  const copyCanvases = Array.from(
    copy.querySelectorAll<HTMLCanvasElement>("canvas")
  );

  for (const [index, sourceCanvas] of sourceCanvases.entries()) {
    if (sourceCanvas.width === 0 || sourceCanvas.height === 0) continue;

    const context = copyCanvases[index]?.getContext("2d") ?? null;
    if (context === null) {
      throw new Error(
        `Unable to copy canvas ${index + 1} for slide ${JSON.stringify(key)}: 2d context unavailable`
      );
    }
    try {
      context.drawImage(sourceCanvas, 0, 0);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      throw new Error(
        `Unable to copy canvas ${index + 1} for slide ${JSON.stringify(key)}: drawImage failed: ${message}`,
        { cause: error }
      );
    }
  }
}

export function expandRevealStepPages(
  doc: Document,
  slides: ReadonlyArray<PdfStepPageSlide>
): void {
  const wraps = Array.from(
    doc.querySelectorAll<HTMLElement>(".peitho-slide-wrap")
  );
  if (wraps.length !== slides.length) {
    throw new Error(
      `PDF slide wrap count ${wraps.length} does not match Rust slide metadata count ${slides.length}`
    );
  }

  for (const [index, wrap] of wraps.entries()) {
    const slide = wrap.querySelector<HTMLElement>("section.peitho-slide");
    if (slide === null) {
      throw new Error(
        `PDF slide wrap ${index + 1} does not contain section.peitho-slide`
      );
    }

    const metadata = slides[index];
    const actualKey = slide.dataset.slideKey;
    if (actualKey !== metadata.key) {
      throw new Error(
        `PDF slide wrap ${index + 1} has data-slide-key ${JSON.stringify(actualKey ?? "<missing>")}; Rust metadata expects ${JSON.stringify(metadata.key)}`
      );
    }

    const stepCount = validatedRevealStepCount(metadata);
    if (stepCount === 0) continue;

    for (let step = 0; step < stepCount; step += 1) {
      const copy = wrap.cloneNode(true) as HTMLElement;
      copyCanvasBitmaps(wrap, copy, metadata.key);
      // Other script-set property values and later async changes reach only the final page.
      applyStepState(copy, step);
      wrap.before(copy);
    }
    applyStepState(wrap, stepCount);
  }
}
