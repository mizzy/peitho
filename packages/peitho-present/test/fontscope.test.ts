import { expect, it } from "vitest";
import { extractFontScopeCss } from "../src/fontscope";

it("extracts leading imports after charset comments and whitespace", () => {
  const css = `
/* deck fonts */
@charset "UTF-8";

@import url("fonts/noto-sans-jp/index.css");
@import url("fonts/inter/index.css") screen;
.peitho-slide { color: red; }
`;

  expect(extractFontScopeCss(css)).toBe(
    [
      '@import url("fonts/noto-sans-jp/index.css");',
      '@import url("fonts/inter/index.css") screen;'
    ].join("\n")
  );
});

it("does not promote imports after ordinary rules", () => {
  const css = `
@import url("fonts/prefix.css");
.peitho-slide { color: red; }
@import url("fonts/late.css");
@font-face { font-family: "Late"; src: url("fonts/late.woff2"); }
`;

  expect(extractFontScopeCss(css)).toBe(
    [
      '@import url("fonts/prefix.css");',
      '@font-face { font-family: "Late"; src: url("fonts/late.woff2"); }'
    ].join("\n")
  );
});

it("extracts top level font face blocks from anywhere", () => {
  const css = `
.slot-title { color: red; }
@font-face { font-family: "Heading"; src: url("fonts/heading.woff2"); }
.slot-body { color: blue; }
@font-face {
  font-family: "Body";
  src: url("fonts/body.woff2");
}
`;

  expect(extractFontScopeCss(css)).toBe(
    [
      '@font-face { font-family: "Heading"; src: url("fonts/heading.woff2"); }',
      '@font-face {\n  font-family: "Body";\n  src: url("fonts/body.woff2");\n}'
    ].join("\n")
  );
});

it("skips comments and strings while scanning font face blocks", () => {
  const css = `
.fake::before { content: "@font-face { nope }"; }
/* @font-face { font-family: "Comment"; } */
@font-face {
  font-family: "Brace } Face";
  src: url("fonts/{brace}.woff2");
  unicode-range: U+0-5FF; /* } */
}
`;

  expect(extractFontScopeCss(css)).toBe(
    [
      "@font-face {",
      '  font-family: "Brace } Face";',
      '  src: url("fonts/{brace}.woff2");',
      "  unicode-range: U+0-5FF; /* } */",
      "}"
    ].join("\n")
  );
});

it("omits non font rules", () => {
  const css = `
@media screen {
  @font-face { font-family: "Nested"; src: url("fonts/nested.woff2"); }
}
.peitho-slide { font-family: "Nested"; }
`;

  expect(extractFontScopeCss(css)).toBe("");
});
