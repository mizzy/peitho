export interface WaitForFontsReadyOptions {
  timeoutMs?: number;
  log?: Pick<Console, "warn">;
}

const MAX_FONT_READY_PASSES = 5;

export async function waitForFontsReady(
  doc: Document,
  win: Window,
  options?: WaitForFontsReadyOptions
): Promise<void> {
  const fonts = doc.fonts;
  if (fonts == null) return;

  const timeoutMs = options?.timeoutMs ?? 3000;
  const deadline = Date.now() + timeoutMs;
  const kicked = new WeakSet<FontFace>();

  for (let pass = 0; pass < MAX_FONT_READY_PASSES; pass += 1) {
    const hasNewFace = kickVisibleFontFaces(fonts, kicked);
    if (!hasNewFace && pass > 0) return;

    const remainingMs = deadline - Date.now();
    if (remainingMs <= 0 || (await raceReadyWithTimeout(fonts, win, remainingMs))) {
      const log = options?.log ?? console;
      log.warn(`document.fonts.ready timed out after ${timeoutMs}ms`);
      return;
    }
  }
}

function kickVisibleFontFaces(fonts: FontFaceSet, kicked: WeakSet<FontFace>): boolean {
  let hasNewFace = false;
  fonts.forEach((face) => {
    if (kicked.has(face)) return;
    kicked.add(face);
    hasNewFace = true;
    try {
      face.load().catch(() => undefined);
    } catch {
      // Ignore invalid or already-failed faces; one broken font must not abort mounting.
    }
  });
  return hasNewFace;
}

async function raceReadyWithTimeout(fonts: FontFaceSet, win: Window, ms: number): Promise<boolean> {
  let timeoutId: number | undefined;
  const timeout = new Promise<"timeout">((resolve) => {
    timeoutId = win.setTimeout(() => resolve("timeout"), ms);
  });

  const ready = fonts.ready.then(
    () => "ready" as const,
    () => "ready" as const
  );
  const result = await Promise.race([ready, timeout]);
  if (timeoutId !== undefined) win.clearTimeout(timeoutId);
  return result === "timeout";
}
