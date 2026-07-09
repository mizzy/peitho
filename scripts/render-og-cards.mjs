#!/usr/bin/env node
import { createReadStream, existsSync, statSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const DEMO_OUT = join(ROOT, ".demo-site");
const OUT_DIR = join(DEMO_OUT, "og");
const HOST = "127.0.0.1";
const PORT = 8766;
const SETTLE_MS = 300;

const CARDS = ["brand", "guide", "examples"];

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
};

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

async function renderCard(browser, name) {
  const context = await browser.newContext({
    viewport: { width: 1200, height: 630 },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  const url = `http://${HOST}:${PORT}/og/${name}.html`;
  const outPath = join(OUT_DIR, `${name}.png`);
  try {
    console.log(`render ${url} -> ${outPath}`);
    const response = await page.goto(url, { waitUntil: "load", timeout: 30_000 });
    if (!response || !response.ok()) {
      throw new Error(`failed to load ${url} (${response?.status() ?? "no response"})`);
    }
    await page.waitForTimeout(SETTLE_MS);
    await page.screenshot({
      path: outPath,
      type: "png",
      fullPage: false,
      clip: { x: 0, y: 0, width: 1200, height: 630 },
    });
  } finally {
    await context.close();
  }
}

async function main() {
  await mkdir(OUT_DIR, { recursive: true });
  const server = await startStaticServer();
  let browser;
  try {
    browser = await chromium.launch();
    for (const name of CARDS) {
      await renderCard(browser, name);
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
