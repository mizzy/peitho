import { build } from "esbuild";

const shared = {
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022"
};

await build({
  ...shared,
  entryPoints: ["src/index.ts"],
  outfile: "dist/shell.js",
  sourcemap: true
});

await build({
  ...shared,
  entryPoints: ["src/preview.ts"],
  outfile: "dist/preview.js",
  sourcemap: true
});

await build({
  ...shared,
  entryPoints: ["src/remote.ts"],
  outfile: "dist/remote.js",
  sourcemap: true
});
