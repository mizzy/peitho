#!/usr/bin/env node
// Records the scripted demo videos committed under site/static/guide-videos/
// (mp4, for the docs site) and docs/images/ (GIF, for the README — GitHub does
// not play repo mp4s):
//   preview-demo    `peitho preview`: grid, single mode, note editing, inline slide editing
//   reveal-demo     `peitho present`: incremental reveal steps
//   emphasis-demo   `peitho present`: stepped code line emphasis
// Usage: node scripts/record-demo-videos.mjs [name...]   (default: all)
// Needs: target/debug/peitho (cargo build -p peitho), playwright, ffmpeg.
import { execFileSync, spawn } from "node:child_process";
import { cp, mkdir, mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const PEITHO = join(ROOT, "target", "debug", "peitho");
const OUT_DIR = join(ROOT, "site", "static", "guide-videos");
const GIF_DIR = join(ROOT, "docs", "images");
// 880px ≈ the README column; 8fps + 64 colors + dropped duplicate frames keeps the tour near 1 MB.
const GIF_FILTER =
  "fps=8,scale=880:-1:flags=lanczos,mpdecimate,split[a][b];" +
  "[a]palettegen=max_colors=64:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle";
const PORT = 8766;
const ORIGIN = `http://localhost:${PORT}`;
const VIEWPORT = { width: 1600, height: 900 };
const TYPE_DELAY_MS = 45;

// Playwright videos do not show the mouse pointer; draw one so clicks read.
const FAKE_CURSOR = `
  addEventListener("DOMContentLoaded", () => {
    const dot = document.createElement("div");
    dot.style.cssText =
      "position:fixed;z-index:2147483647;width:22px;height:22px;margin:-11px 0 0 -11px;" +
      "border-radius:50%;background:rgba(255,80,80,.55);border:2px solid #fff;" +
      "pointer-events:none;left:-99px;top:-99px;transition:left .5s ease,top .5s ease";
    document.body.appendChild(dot);
    addEventListener("mousemove", (e) => {
      dot.style.left = e.clientX + "px";
      dot.style.top = e.clientY + "px";
    }, true);
  });
`;

async function waitForServer(url) {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`peitho did not come up on ${url}`);
}

// Every tour returns the moment its first real frame was on screen; the blank
// page before it is trimmed from the video.
async function opened(page, url, readySelector) {
  await page.goto(url, { waitUntil: "load" });
  await page.locator(readySelector).first().waitFor();
  await page.waitForTimeout(300);
  return Date.now();
}

async function previewTour(page) {
  const beat = (ms = 1200) => page.waitForTimeout(ms);
  const gridShownAt = await opened(page, `${ORIGIN}/`, ".peitho-preview-number");
  await beat(2000); // grid overview

  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("ArrowRight");
    await beat(450);
  }
  await page.keyboard.press("Enter"); // open the selected slide (single mode)
  await beat(1800);

  await page.keyboard.press("Enter"); // focus the speaker note
  await beat(600);
  await page.keyboard.type(" Preview is where a deck spends most of its life.", {
    delay: TYPE_DELAY_MS,
  });
  await beat();
  await page.keyboard.press("Escape"); // blur the note, which autosaves it
  await beat(800);

  await inlineEdit(page, '[data-peitho-md^="**Preview**"]', " **No manual refresh** — see [the docs](https://peitho.gosu.ke/).");
  await page.keyboard.press("PageDown");
  await beat(1500);
  await inlineEdit(page, 'h1 [data-peitho-md="Install"]', " in one line");

  await page.keyboard.press("Escape"); // back to the grid, edited title visible
  await beat(2500);
  return gridShownAt;
}

// Click a rendered block, append to its Markdown source, Enter commits.
async function inlineEdit(page, selector, appended) {
  const beat = (ms) => page.waitForTimeout(ms);
  // The filmstrip thumbnail carries the same annotation; the stage copy is the wide one.
  const copies = await page.locator(selector).all();
  const boxes = await Promise.all(copies.map((copy) => copy.boundingBox()));
  const widest = boxes.reduce((best, b, i) => ((b?.width ?? 0) > (boxes[best]?.width ?? 0) ? i : best), 0);
  const box = boxes[widest];
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await beat(900);
  await copies[widest].click();
  await page.mouse.move(box.x + box.width + 100, box.y + 300); // keep the pointer off the text being edited
  await beat(700);
  await page.keyboard.press("Meta+ArrowDown"); // caret to the end of the source
  await page.keyboard.type(appended, { delay: TYPE_DELAY_MS });
  await beat(800);
  await page.keyboard.press("Enter");
  // write → watch rebuild → reload; hold on the rendered result so the viewer can read it
  await page.locator(`[data-peitho-md$=${JSON.stringify(appended)}]`).first().waitFor({ state: "attached" });
  await beat(3500);
}

// Walks `slides` slides of a present deck one reveal step at a time.
async function presentTour(page, slides) {
  const beat = (ms) => page.waitForTimeout(ms);
  const shownAt = await opened(page, `${ORIGIN}/present.html`, "h1");
  const manifest = await (await fetch(`${ORIGIN}/manifest.json`)).json();
  await beat(1500);
  for (const [i, slide] of manifest.slides.slice(0, slides).entries()) {
    for (let step = 0; step < slide.revealSteps; step++) {
      await page.keyboard.press("ArrowRight");
      await beat(1300);
    }
    await beat(700);
    if (i < slides - 1) {
      await page.keyboard.press("ArrowRight");
      await beat(1500);
    }
  }
  return shownAt;
}

const VIDEOS = {
  "preview-demo": { deck: "peitho-tour", command: "preview", tour: previewTour, readme: true },
  "reveal-demo": { deck: "incremental-reveal", command: "present", tour: (page) => presentTour(page, 2), readme: true },
  "emphasis-demo": { deck: "code-emphasis", command: "present", tour: (page) => presentTour(page, 2) },
};

async function record(name) {
  const { deck, command, tour, readme } = VIDEOS[name];
  const out = join(OUT_DIR, `${name}.mp4`);
  const work = await mkdtemp(join(tmpdir(), "peitho-preview-demo-"));
  const deckDir = join(work, "deck");
  const videoDir = join(work, "video");
  // The preview tour edits the deck source, so record against a throwaway copy.
  await cp(join(ROOT, "examples", deck), deckDir, { recursive: true });

  const server = spawn(PEITHO, [command, "deck.md", "--port", String(PORT), "--no-open"], {
    cwd: deckDir,
    stdio: "inherit",
  });
  let browser;
  try {
    await waitForServer(`${ORIGIN}/manifest.json`);
    browser = await chromium.launch();
    const context = await browser.newContext({
      viewport: VIEWPORT,
      recordVideo: { dir: videoDir, size: VIEWPORT },
    });
    await context.addInitScript(FAKE_CURSOR);
    const recordingStartedAt = Date.now();
    const shownAt = await tour(await context.newPage());
    await context.close(); // flushes the webm

    const [webm] = await readdir(videoDir);
    await mkdir(OUT_DIR, { recursive: true });
    execFileSync("ffmpeg", [
      "-y", "-loglevel", "error", "-i", join(videoDir, webm),
      // +0.6s: the encoder lags the page clock, so the measured offset alone leaves a black frame
      "-ss", ((shownAt - recordingStartedAt) / 1000 + 0.6).toFixed(2),
      "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-movflags", "+faststart", out,
    ]);
    console.log(`recorded ${out}`);
    if (readme) {
      const gif = join(GIF_DIR, `${name}.gif`);
      execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-i", out, "-vf", GIF_FILTER, "-vsync", "vfr", gif]);
      console.log(`recorded ${gif}`);
    }
  } finally {
    if (browser) await browser.close();
    server.kill();
    await new Promise((resolve) => server.once("exit", resolve)); // free the port for the next video
    await rm(work, { recursive: true, force: true });
  }
}

async function main() {
  const names = process.argv.slice(2).length > 0 ? process.argv.slice(2) : Object.keys(VIDEOS);
  for (const name of names) {
    if (!VIDEOS[name]) throw new Error(`unknown video "${name}" (known: ${Object.keys(VIDEOS).join(", ")})`);
    await record(name);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
