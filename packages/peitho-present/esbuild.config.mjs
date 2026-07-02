import { build } from "esbuild";

await build({
  entryPoints: ["src/index.ts"],
  outfile: "dist/shell.js",
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  sourcemap: true
});
