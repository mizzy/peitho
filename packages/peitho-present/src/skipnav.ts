export function nextNonSkippedIndex(
  slides: ReadonlyArray<{ skip?: boolean }>,
  from: number,
  direction: 1 | -1
): number | null {
  let index = from + direction;
  while (index >= 0 && index < slides.length) {
    if (slides[index].skip !== true) return index;
    index += direction;
  }
  return null;
}

export function initialSlideIndex(slides: ReadonlyArray<{ skip?: boolean }>): number | null {
  if (slides.length === 0) return null;
  return nextNonSkippedIndex(slides, -1, 1) ?? 0;
}

export function presentedBefore(slides: ReadonlyArray<{ skip?: boolean }>, index: number): number {
  let count = 0;
  for (let i = 0; i < slides.length && i < index; i += 1) {
    if (slides[i].skip !== true) count += 1;
  }
  return count;
}

export function slideProgress(slides: ReadonlyArray<{ skip?: boolean }>, index: number): number {
  const count = presentedBefore(slides, slides.length);
  if (count <= 1) return 1;
  const position = presentedBefore(slides, index) - (slides[index]?.skip === true ? 1 : 0);
  return Math.min(1, Math.max(0, position / (count - 1)));
}
