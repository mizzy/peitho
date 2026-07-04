import type { ManifestSection } from "../../../bindings/ManifestSection";

export function sectionIndexForSlide(
  sections: ManifestSection[],
  slideIndex: number
): number {
  return sections.findIndex(
    (section) => slideIndex >= section.startIndex && slideIndex <= section.endIndex
  );
}
