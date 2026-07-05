export type CanvasViewport = {
  width: number;
  height: number;
};

export type CanvasFit = {
  scale: number;
  width: number;
  height: number;
  left: number;
  top: number;
};

export type CanvasScalerOptions = {
  target: HTMLElement;
  window?: Window;
  viewport?: () => CanvasViewport;
  canvasWidth: number;
  canvasHeight: number;
};

export function calculateCanvasFit(
  viewport: CanvasViewport,
  canvasWidth: number,
  canvasHeight: number
): CanvasFit {
  const scale = Math.min(viewport.width / canvasWidth, viewport.height / canvasHeight);
  const width = canvasWidth * scale;
  const height = canvasHeight * scale;
  return {
    scale,
    width,
    height,
    left: (viewport.width - width) / 2,
    top: (viewport.height - height) / 2
  };
}

export function installCanvasScaler(options: CanvasScalerOptions): () => void {
  const win = options.window ?? window;
  const canvasWidth = options.canvasWidth;
  const canvasHeight = options.canvasHeight;
  const viewport =
    options.viewport ??
    (() => ({
      width: win.innerWidth,
      height: win.innerHeight
    }));

  function apply(): void {
    const fit = calculateCanvasFit(viewport(), canvasWidth, canvasHeight);
    options.target.style.width = `${canvasWidth}px`;
    options.target.style.height = `${canvasHeight}px`;
    options.target.style.transformOrigin = "top left";
    options.target.style.transform = `translate(${fit.left}px, ${fit.top}px) scale(${fit.scale})`;
  }

  apply();
  win.addEventListener("resize", apply);
  return () => win.removeEventListener("resize", apply);
}
