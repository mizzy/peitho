export type ClickNavigationGuardOptions = {
  target: HTMLElement;
  window?: Window;
  moveThresholdPx?: number;
};

export type ClickNavigationGuard = {
  shouldIgnoreClick(event: MouseEvent): boolean;
  destroy(): void;
};

const DEFAULT_MOVE_THRESHOLD_PX = 5;

type Point = {
  x: number;
  y: number;
};

// Kept in sync with crates/peitho-core/src/render.rs render_distribution_index.
export function createClickNavigationGuard(
  options: ClickNavigationGuardOptions
): ClickNavigationGuard {
  const win = options.window ?? window;
  const moveThresholdPx = options.moveThresholdPx ?? DEFAULT_MOVE_THRESHOLD_PX;
  let clickStart: Point | null = null;

  const onMouseDown = (event: MouseEvent): void => {
    clickStart = { x: event.clientX, y: event.clientY };
  };

  options.target.addEventListener("mousedown", onMouseDown);

  return {
    shouldIgnoreClick(event: MouseEvent): boolean {
      const start = clickStart;
      clickStart = null;
      if (hasNonCollapsedSelection(win)) return true;
      if (start === null) return false;
      return Math.hypot(event.clientX - start.x, event.clientY - start.y) > moveThresholdPx;
    },
    destroy(): void {
      options.target.removeEventListener("mousedown", onMouseDown);
    }
  };
}

function hasNonCollapsedSelection(win: Window): boolean {
  const selection = win.getSelection();
  return selection !== null && !selection.isCollapsed;
}
