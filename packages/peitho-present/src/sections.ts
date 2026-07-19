import type { ManifestSection } from "../../../bindings/ManifestSection";
import { isValidDurationMs } from "./timeTracker";

export function sectionIndexForSlide(
  sections: ManifestSection[],
  slideIndex: number
): number {
  return sections.findIndex(
    (section) => slideIndex >= section.startIndex && slideIndex <= section.endIndex
  );
}

export function validateSections(
  sections: ManifestSection[],
  log: Pick<Console, "error">
): boolean {
  let expectedStartIndex = 0;
  for (const [index, section] of sections.entries()) {
    const label = `manifest section ${index + 1} "${section.name}"`;
    if (!isValidDurationMs(section.plannedDurationMs)) {
      log.error(`Invalid plannedDurationMs for ${label} in manifest.json`);
      return false;
    }
    if (!isValidSlideIndex(section.startIndex) || !isValidSlideIndex(section.endIndex)) {
      log.error(
        `Invalid ${label}: startIndex and endIndex must be non-negative integers`
      );
      return false;
    }
    if (section.endIndex < section.startIndex) {
      log.error(`Invalid ${label}: endIndex must be greater than or equal to startIndex`);
      return false;
    }
    if (section.startIndex !== expectedStartIndex) {
      log.error(
        `Invalid ${label}: expected startIndex ${expectedStartIndex}, got ${section.startIndex}`
      );
      return false;
    }
    expectedStartIndex = section.endIndex + 1;
  }
  return true;
}

function isValidSlideIndex(index: number): boolean {
  return Number.isSafeInteger(index) && index >= 0;
}
