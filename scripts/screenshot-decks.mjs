#!/usr/bin/env node
import { createReadStream, existsSync, statSync } from "node:fs";
import { mkdir, readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const DEMO_OUT = join(ROOT, ".demo-site");
const SHOTS_DIR = join(DEMO_OUT, "deck-shots");
const HOST = "127.0.0.1";
const PORT = 8765;
const SETTLE_MS = 500;

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".mjs": "application/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
  ".pdf": "application/pdf",
};

function parseMakeWords(value) {
  return value.split(/\s+/).map((word) => word.trim()).filter(Boolean);
}

async function readDemoDecksFromMakefile() {
  const makefile = await readFile(join(ROOT, "Makefile"), "utf8");
  const lines = makefile.split(/\r?\n/);
  let value = "";
  let collecting = false;

  for (const line of lines) {
    if (!collecting) {
      const match = line.match(/^DEMO_DECKS\s*(?:[:+?]?=)\s*(.*)$/);
      if (!match) continue;
      value += match[1].replace(/\\\s*$/, " ");
      collecting = /\\\s*$/.test(match[1]);
      if (!collecting) break;
      continue;
    }

    value += line.replace(/\\\s*$/, " ");
    collecting = /\\\s*$/.test(line);
    if (!collecting) break;
  }

  const decks = parseMakeWords(value);
  if (decks.length === 0) {
    throw new Error("could not find DEMO_DECKS in Makefile");
  }
  return decks;
}

function resolveRequestPath(req) {
  const url = new URL(req.url ?? "/", `http://${HOST}:${PORT}`);
  const decoded = decodeURIComponent(url.pathname);
  const rel = normalize(decoded).replace(/^([/\\])+/, "");
  if (rel.startsWith("..") || rel.split(sep).includes("..")) {
    return null;
  }
  return join(DEMO_OUT, rel);
}

function startStaticServer() {
  return new Promise((resolve, reject) => {
    const server = createServer((req, res) => {
      const requested = resolveRequestPath(req);
      if (!requested) {
        res.statusCode = 400;
        res.end("bad path");
        return;
      }

      let filePath = requested;
      let stat;
      try {
        stat = statSync(filePath);
      } catch {
        res.statusCode = 404;
        res.end("not found");
        return;
      }

      if (stat.isDirectory()) {
        filePath = join(filePath, "index.html");
        if (!existsSync(filePath)) {
          res.statusCode = 404;
          res.end("not found");
          return;
        }
      }

      res.setHeader("content-type", MIME[extname(filePath)] || "application/octet-stream");
      createReadStream(filePath).pipe(res);
    });

    server.once("error", reject);
    server.listen(PORT, HOST, () => resolve(server));
  });
}

function closeServer(server) {
  if (!server) return Promise.resolve();
  return new Promise((resolve, reject) => {
    server.close((err) => (err ? reject(err) : resolve()));
  });
}

function viewportFor(deck) {
  if (deck === "aspect-ratio-4-3") {
    return { width: 1200, height: 900 };
  }
  return { width: 1600, height: 900 };
}

async function screenshotDeck(browser, deck) {
  const viewport = viewportFor(deck);
  const context = await browser.newContext({ viewport });
  const page = await context.newPage();
  const url = `http://${HOST}:${PORT}/demo/${encodeURIComponent(deck)}/index.html`;
  const shotPath = join(SHOTS_DIR, `${deck}.png`);

  try {
    console.log(`screenshot ${deck} -> ${shotPath}`);
    const response = await page.goto(url, { waitUntil: "load", timeout: 30_000 });
    if (!response || !response.ok()) {
      throw new Error(`${deck}: failed to load ${url} (${response?.status() ?? "no response"})`);
    }
    await page.waitForTimeout(SETTLE_MS);
    await page.screenshot({ path: shotPath, type: "png", fullPage: false });
  } finally {
    await context.close();
  }
}

async function main() {
  const decks = process.argv.slice(2).length > 0
    ? parseMakeWords(process.argv.slice(2).join(" "))
    : await readDemoDecksFromMakefile();
  if (decks.length === 0) {
    throw new Error("no decks to screenshot");
  }

  await mkdir(SHOTS_DIR, { recursive: true });
  const server = await startStaticServer();
  let browser;
  try {
    browser = await chromium.launch();
    for (const deck of decks) {
      await screenshotDeck(browser, deck);
    }
  } finally {
    if (browser) {
      await browser.close();
    }
    await closeServer(server);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
