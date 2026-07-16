export function nextNonSkippedIndex(
  slides: ReadonlyArray<{ skip: boolean }>,
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

export function initialSlideIndex(slides: ReadonlyArray<{ skip: boolean }>): number | null {
  if (slides.length === 0) return null;
  return nextNonSkippedIndex(slides, -1, 1) ?? 0;
}
