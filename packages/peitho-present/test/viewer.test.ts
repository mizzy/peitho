import { expect, it } from "vitest";
import * as viewer from "../src/viewer";
import {
  createViewerNavigator,
  readViewerPosition,
  writeViewerPosition
} from "../src/viewer";

it("exports exactly the distribution viewer runtime surface", () => {
  expect(Object.keys(viewer).sort()).toEqual([
    "announceParsedSlides",
    "announceShadowMounted",
    "applyStepState",
    "createStepStateStyle",
    "createViewerNavigator",
    "dropDisconnectedShadowMounted",
    "executeInlineScripts",
    "keyBelongsToTarget",
    "pointerBelongsToTarget",
    "readViewerPosition",
    "shadowMountedBacklog",
    "writeViewerPosition"
  ]);
});

it("navigates reveal steps and round-trips mode-specific URL positions", () => {
  const stepped = createViewerNavigator({
    slides: [{ revealSteps: 1 }, { revealSteps: 2 }],
    walkRevealSteps: true
  });
  expect(stepped.move({ index: 0, step: 0 }, 1)).toEqual({ index: 0, step: 1 });
  expect(stepped.move({ index: 0, step: 1 }, 1)).toEqual({ index: 1, step: 0 });
  expect(stepped.move({ index: 1, step: 0 }, -1)).toEqual({ index: 0, step: 1 });
  expect(stepped.move({ index: 0, step: 0 }, -1)).toEqual({ index: 0, step: 0 });
  expect(stepped.move({ index: 1, step: 2 }, 1)).toEqual({ index: 1, step: 2 });

  expect(readViewerPosition(new URL("https://deck.test/?slide=2"), stepped)).toEqual({
    index: 1,
    step: 2
  });
  expect(readViewerPosition(new URL("https://deck.test/?slide=2&step=bad"), stepped)).toEqual({
    index: 1,
    step: 2
  });
  expect(readViewerPosition(new URL("https://deck.test/?slide=2&step=99"), stepped)).toEqual({
    index: 1,
    step: 2
  });
  expect(readViewerPosition(new URL("https://deck.test/?slide=2&step=1"), stepped)).toEqual({
    index: 1,
    step: 1
  });
  expect(readViewerPosition(new URL("https://deck.test/#slide=2"), stepped)).toEqual({
    index: 1,
    step: 2
  });
  for (const raw of ["", "-1", "1.5", "x", "NaN", "Infinity", "9007199254740992"]) {
    expect(
      readViewerPosition(
        new URL(`https://deck.test/?slide=2&step=${encodeURIComponent(raw)}`),
        stepped
      )
    ).toEqual({ index: 1, step: 2 });
  }

  const final = createViewerNavigator({
    slides: [{ revealSteps: 1 }, { revealSteps: 2 }],
    walkRevealSteps: false
  });
  expect(readViewerPosition(new URL("https://deck.test/?slide=2&step=0"), final)).toEqual({
    index: 1,
    step: 2
  });
  expect(final.direct(0, 0)).toEqual({ index: 0, step: 1 });
  expect(final.move({ index: 0, step: 0 }, 1)).toEqual({ index: 1, step: 2 });
  expect(final.move({ index: 1, step: 0 }, 1)).toEqual({ index: 1, step: 2 });
  expect(
    writeViewerPosition(
      new URL("https://deck.test/?theme=dark&step=0#notes"),
      { index: 1, step: 2 },
      final
    ).href
  ).toBe("https://deck.test/?theme=dark&slide=2#notes");
  expect(
    writeViewerPosition(
      new URL("https://deck.test/?theme=dark#notes"),
      { index: 1, step: 1 },
      stepped
    ).href
  ).toBe("https://deck.test/?theme=dark&slide=2&step=1#notes");

  const skipMetadata = [
    { revealSteps: 0 },
    { revealSteps: 0, skip: true },
    { revealSteps: 0 }
  ];
  const doesNotSkip = createViewerNavigator({
    slides: skipMetadata,
    walkRevealSteps: true
  });
  expect(doesNotSkip.move({ index: 0, step: 0 }, 1)).toEqual({ index: 1, step: 0 });
});
