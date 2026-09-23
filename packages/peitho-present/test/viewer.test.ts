import { expect, it } from "vitest";
import * as viewer from "../src/viewer";

it("exports exactly the distribution viewer runtime surface", () => {
  expect(Object.keys(viewer).sort()).toEqual([
    "announceShadowMounted",
    "dropDisconnectedShadowMounted",
    "executeInlineScripts",
    "keyBelongsToTarget",
    "pointerBelongsToTarget",
    "shadowMountedBacklog"
  ]);
});
