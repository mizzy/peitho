# Peitho Milestone 8 Display Management Plan

## Purpose

Milestone 8 fixes presentation-window behavior reported by author review:

- `peitho present` should open a toolbar-free app-mode browser window and request fullscreen through browser launch flags where possible.
- The Presenter button should open `presenter.html` as a popup window, and when the Window Management API is available it should place the slide window on a non-current screen while keeping the presenter popup on the current screen.

This milestone does not add a join protocol, startup JavaScript fullscreen, Firefox/Safari multiscreen support, or new CLI flags. Existing `--no-open`, `--no-serve`, `--port`, and `--shell` behavior remains.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `crates/peitho/src/browser.rs` | Pure browser command planning and thin process spawn wrapper | `std::process::Command`, OS probes |
| `crates/peitho/src/main.rs` | Use `browser::open_browser` from `present` and remove inline browser helpers | `browser.rs` |
| `crates/peitho/src/lib.rs` | Export `browser` for the binary and unit tests | `browser.rs` |
| `packages/peitho-present/src/window-management.d.ts` | Minimal Window Management API declarations | DOM |
| `packages/peitho-present/src/presentDisplay.ts` | `openPresenterWithDisplay`, screen selection, popup features, fullscreen request | browser DOM |
| `packages/peitho-present/src/controls.ts` | Default Presenter button action calls `openPresenterWithDisplay`; injection remains | `presentDisplay.ts` |
| `packages/peitho-present/src/index.ts` | Export display-management API and types | local TS modules |
| `packages/peitho-present/test/presentDisplay.test.ts` | Deterministic Window Management API tests | `presentDisplay.ts` |
| `packages/peitho-present/test/controls.test.ts` | Presenter button default opens popup through display helper | `controls.ts` |
| `packages/peitho-present/test/generated.test.ts` | Public export smoke for display helper | `index.ts` |
| `packages/peitho-present/tsconfig.json` | Include `.d.ts` declarations | TypeScript |

Dependency order:

1. Extract Rust browser command planning into a pure module.
2. Wire `present` to the new browser module without changing CLI flags.
3. Add minimal Window Management API types.
4. Implement and test `openPresenterWithDisplay`.
5. Make controls use the display helper by default.
6. Export the public TS API.
7. Run full Rust/TS gates and smoke-check generated `present.html`.

## Implementation Tasks

### Task 1 - Add Pure Browser Command Planning for macOS

Goal: build app-mode Chrome commands on macOS when Google Chrome is available, with `open <URL>` fallback.

Files:

- `crates/peitho/src/browser.rs`
- `crates/peitho/src/lib.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn macos_uses_google_chrome_app_mode_when_available() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: true,
            linux_browser: None,
        };

        let command = plan_browser_command("http://127.0.0.1:8000/present.html", &env).unwrap();

        assert_eq!(command.program, OsString::from("open"));
        assert_eq!(
            command.args,
            vec![
                OsString::from("-na"),
                OsString::from("Google Chrome"),
                OsString::from("--args"),
                OsString::from("--app=http://127.0.0.1:8000/present.html"),
                OsString::from("--start-fullscreen"),
            ]
        );
    }

    #[test]
    fn macos_falls_back_to_open_when_google_chrome_is_absent() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: false,
            linux_browser: None,
        };

        let command = plan_browser_command("http://127.0.0.1:8000/present.html", &env).unwrap();

        assert_eq!(command.program, OsString::from("open"));
        assert_eq!(
            command.args,
            vec![OsString::from("http://127.0.0.1:8000/present.html")]
        );
    }
}
```

Expected Red:

```text
file not found for module `browser`
```

Implementation:

```rust
// crates/peitho/src/lib.rs
pub mod browser;
pub mod server;
```

```rust
// crates/peitho/src/browser.rs
use std::{ffi::OsString, path::Path, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserPlatform {
    Macos,
    Linux,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserEnvironment {
    pub platform: BrowserPlatform,
    pub mac_google_chrome_available: bool,
    pub linux_browser: Option<OsString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
}

pub fn plan_browser_command(url: &str, env: &BrowserEnvironment) -> Option<BrowserCommand> {
    match env.platform {
        BrowserPlatform::Macos if env.mac_google_chrome_available => Some(BrowserCommand {
            program: OsString::from("open"),
            args: vec![
                OsString::from("-na"),
                OsString::from("Google Chrome"),
                OsString::from("--args"),
                OsString::from(format!("--app={url}")),
                OsString::from("--start-fullscreen"),
            ],
        }),
        BrowserPlatform::Macos => Some(BrowserCommand {
            program: OsString::from("open"),
            args: vec![OsString::from(url)],
        }),
        BrowserPlatform::Linux => linux_browser_command(url, env.linux_browser.as_deref()),
        BrowserPlatform::Other => None,
    }
}

fn linux_browser_command(url: &str, browser: Option<&std::ffi::OsStr>) -> Option<BrowserCommand> {
    match browser {
        Some(program) => Some(BrowserCommand {
            program: program.to_owned(),
            args: vec![OsString::from(format!("--app={url}")), OsString::from("--start-fullscreen")],
        }),
        None => Some(BrowserCommand {
            program: OsString::from("xdg-open"),
            args: vec![OsString::from(url)],
        }),
    }
}

fn chrome_app_exists() -> bool {
    Path::new("/Applications/Google Chrome.app").exists()
}

fn current_platform() -> BrowserPlatform {
    if cfg!(target_os = "macos") {
        BrowserPlatform::Macos
    } else if cfg!(target_os = "linux") {
        BrowserPlatform::Linux
    } else {
        BrowserPlatform::Other
    }
}

fn find_linux_browser() -> Option<OsString> {
    find_in_path("google-chrome").or_else(|| find_in_path("chromium"))
}

fn find_in_path(program: &str) -> Option<OsString> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(program);
        candidate.is_file().then(|| OsString::from(program))
    })
}

fn current_environment() -> BrowserEnvironment {
    BrowserEnvironment {
        platform: current_platform(),
        mac_google_chrome_available: chrome_app_exists(),
        linux_browser: find_linux_browser(),
    }
}

pub fn open_browser(url: &str) {
    let env = current_environment();
    let Some(command) = plan_browser_command(url, &env) else {
        eprintln!("warning: browser auto-open is not supported on this platform");
        return;
    };
    if let Err(err) = Command::new(&command.program).args(&command.args).spawn() {
        eprintln!(
            "warning: failed to open browser with {}: {err}",
            command.program.to_string_lossy()
        );
    }
}
```

Verification:

```sh
cargo test -p peitho browser::tests::macos_uses_google_chrome_app_mode_when_available
cargo test -p peitho browser::tests::macos_falls_back_to_open_when_google_chrome_is_absent
```

### Task 2 - Add Linux Browser Planning Tests

Goal: Linux uses Chrome/Chromium app mode when available and `xdg-open` otherwise.

Files:

- `crates/peitho/src/browser.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn linux_uses_google_chrome_app_mode_when_available() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Linux,
        mac_google_chrome_available: false,
        linux_browser: Some(OsString::from("google-chrome")),
    };

    let command = plan_browser_command("http://127.0.0.1:9000/present.html", &env).unwrap();

    assert_eq!(command.program, OsString::from("google-chrome"));
    assert_eq!(
        command.args,
        vec![
            OsString::from("--app=http://127.0.0.1:9000/present.html"),
            OsString::from("--start-fullscreen")
        ]
    );
}

#[test]
fn linux_uses_chromium_app_mode_when_chromium_is_the_available_browser() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Linux,
        mac_google_chrome_available: false,
        linux_browser: Some(OsString::from("chromium")),
    };

    let command = plan_browser_command("http://127.0.0.1:9000/present.html", &env).unwrap();

    assert_eq!(command.program, OsString::from("chromium"));
    assert_eq!(
        command.args,
        vec![
            OsString::from("--app=http://127.0.0.1:9000/present.html"),
            OsString::from("--start-fullscreen")
        ]
    );
}

#[test]
fn linux_falls_back_to_xdg_open_without_chrome_or_chromium() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Linux,
        mac_google_chrome_available: false,
        linux_browser: None,
    };

    let command = plan_browser_command("http://127.0.0.1:9000/present.html", &env).unwrap();

    assert_eq!(command.program, OsString::from("xdg-open"));
    assert_eq!(
        command.args,
        vec![OsString::from("http://127.0.0.1:9000/present.html")]
    );
}

#[test]
fn unsupported_platform_returns_no_command() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Other,
        mac_google_chrome_available: false,
        linux_browser: None,
    };

    assert!(plan_browser_command("http://127.0.0.1:9000/present.html", &env).is_none());
}
```

Implementation:

- Use the `linux_browser_command` branch from Task 1.
- Keep program selection outside `plan_browser_command`; the pure function receives `linux_browser`.

Verification:

```sh
cargo test -p peitho browser::tests::linux_uses_google_chrome_app_mode_when_available
cargo test -p peitho browser::tests::linux_uses_chromium_app_mode_when_chromium_is_the_available_browser
cargo test -p peitho browser::tests::linux_falls_back_to_xdg_open_without_chrome_or_chromium
cargo test -p peitho browser::tests::unsupported_platform_returns_no_command
```

### Task 3 - Wire Present Command to Browser Module

Goal: `present` uses the new browser opener and no browser-specific helpers remain in `main.rs`.

Files:

- `crates/peitho/src/browser.rs`
- `crates/peitho/src/main.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn current_environment_matches_supported_platform_shape() {
    let env = current_environment();
    if cfg!(target_os = "macos") {
        assert_eq!(env.platform, BrowserPlatform::Macos);
    } else if cfg!(target_os = "linux") {
        assert_eq!(env.platform, BrowserPlatform::Linux);
    } else {
        assert_eq!(env.platform, BrowserPlatform::Other);
    }
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
use peitho::{browser, server};

// in present()
if !options.no_open {
    browser::open_browser(&url);
}
```

Delete these from `main.rs`; do not keep wrappers around the new module, because they become dead code under `clippy -D warnings`:

```rust
fn browser_command() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("open")
    } else if cfg!(target_os = "linux") {
        Some("xdg-open")
    } else {
        None
    }
}

fn open_browser(url: &str) {
    let Some(command) = browser_command() else {
        eprintln!("warning: browser auto-open is not supported on this platform");
        return;
    };
    if let Err(err) = std::process::Command::new(command).arg(url).spawn() {
        eprintln!("warning: failed to open browser with {command}: {err}");
    }
}
```

Delete the old `browser_command_matches_supported_platforms` test from `main.rs`. The browser module tests own command selection after this task.

Verification:

```sh
cargo test -p peitho browser::tests::current_environment_matches_supported_platform_shape
cargo test -p peitho --test present present_no_open_server_prints_assigned_url
```

### Task 4 - Add Minimal Window Management API Types

Goal: TypeScript can reference `window.getScreenDetails()` and fullscreen `screen` options without using `any`.

Files:

- `packages/peitho-present/src/window-management.d.ts`
- `packages/peitho-present/tsconfig.json`
- `packages/peitho-present/test/presentDisplay.test.ts`

Test:

```ts
// packages/peitho-present/test/presentDisplay.test.ts
import { expect, it } from "vitest";

it("compiles the minimal window management types", () => {
  const screen: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const details: ScreenDetails = {
    currentScreen: screen,
    screens: [screen]
  };
  const options: FullscreenOptions = { screen };

  expect(details.currentScreen.availLeft).toBe(1440);
  expect(options.screen).toBe(screen);
});
```

Expected Red:

```text
Cannot find name 'ScreenDetailed'
```

Implementation:

```ts
// packages/peitho-present/src/window-management.d.ts
interface ScreenDetailed {
  availLeft: number;
  availTop: number;
  availWidth: number;
  availHeight: number;
}

interface ScreenDetails {
  currentScreen: ScreenDetailed;
  screens: readonly ScreenDetailed[];
}

interface FullscreenOptions {
  screen?: ScreenDetailed;
}

interface Window {
  getScreenDetails?: () => Promise<ScreenDetails>;
}
```

```json
// packages/peitho-present/tsconfig.json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2022", "DOM"],
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["vitest/globals"]
  },
  "include": ["src/**/*.ts", "src/**/*.d.ts", "test/**/*.ts", "../../bindings/**/*.ts"]
}
```

Verification:

```sh
cd packages/peitho-present
npm run typecheck
```

### Task 5 - Implement Presenter Popup Feature Builder

Goal: produce deterministic popup features with `left`, `top`, `width`, and `height`.

Files:

- `packages/peitho-present/src/presentDisplay.ts`
- `packages/peitho-present/test/presentDisplay.test.ts`

Test:

```ts
// packages/peitho-present/test/presentDisplay.test.ts
import { buildPresenterFeatures, chooseOtherScreen } from "../src/presentDisplay";

it("builds presenter popup features from the current screen", () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };

  expect(buildPresenterFeatures(current)).toBe("popup=yes,width=1200,height=800,left=0,top=0");
});

it("caps presenter popup size to the current screen", () => {
  const current: ScreenDetailed = {
    availLeft: 20,
    availTop: 30,
    availWidth: 900,
    availHeight: 700
  };

  expect(buildPresenterFeatures(current)).toBe("popup=yes,width=900,height=700,left=20,top=30");
});

it("chooses a non-current screen when one exists", () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };

  expect(chooseOtherScreen({ currentScreen: current, screens: [current, other] })).toBe(other);
});
```

Implementation:

```ts
// packages/peitho-present/src/presentDisplay.ts
export const PRESENTER_URL = "presenter.html";
const PRESENTER_TARGET = "peitho-presenter";
const DEFAULT_POPUP_WIDTH = 1200;
const DEFAULT_POPUP_HEIGHT = 800;

export function buildPresenterFeatures(screen: ScreenDetailed): string {
  const width = Math.min(DEFAULT_POPUP_WIDTH, screen.availWidth);
  const height = Math.min(DEFAULT_POPUP_HEIGHT, screen.availHeight);
  return [
    "popup=yes",
    `width=${width}`,
    `height=${height}`,
    `left=${screen.availLeft}`,
    `top=${screen.availTop}`
  ].join(",");
}

export function fallbackFeatures(): string {
  return "popup=yes,width=1200,height=800,left=80,top=80";
}

export function chooseOtherScreen(details: ScreenDetails): ScreenDetailed | null {
  return details.screens.find((screen) => screen !== details.currentScreen) ?? null;
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- presentDisplay.test.ts
```

### Task 6 - Implement openPresenterWithDisplay

Goal: synchronously secure the presenter popup before any permission prompt, then move/resize it only when multiscreen details are available.

Files:

- `packages/peitho-present/src/presentDisplay.ts`
- `packages/peitho-present/test/presentDisplay.test.ts`

Test:

```ts
// packages/peitho-present/test/presentDisplay.test.ts
import { expect, it, vi } from "vitest";
import { openPresenterWithDisplay } from "../src/presentDisplay";

it("opens the popup before awaiting screen details, then places windows on two screens", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  let resolveDetails: (details: ScreenDetails) => void = () => {};
  const detailsPromise = new Promise<ScreenDetails>((resolve) => {
    resolveDetails = resolve;
  });
  const openWindow = vi.fn(() => popup);
  const requestFullscreen = vi.fn(async () => undefined);

  const pending = openPresenterWithDisplay({
    getScreenDetails: () => detailsPromise,
    openWindow,
    requestFullscreen
  });

  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
  expect(requestFullscreen).not.toHaveBeenCalled();

  resolveDetails({ currentScreen: current, screens: [current, other] });
  await pending;

  expect(requestFullscreen).toHaveBeenCalledWith({ screen: other });
  expect(popup.moveTo).toHaveBeenCalledWith(0, 0);
  expect(popup.resizeTo).toHaveBeenCalledWith(1200, 800);
});

it("falls back to popup only when the api is absent", async () => {
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const openWindow = vi.fn(() => popup);
  const requestFullscreen = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: undefined,
    openWindow,
    requestFullscreen
  });

  expect(requestFullscreen).not.toHaveBeenCalled();
  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(popup.resizeTo).not.toHaveBeenCalled();
});

it("falls back to popup only for a single screen", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const openWindow = vi.fn(() => popup);
  const requestFullscreen = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current] }),
    openWindow,
    requestFullscreen
  });

  expect(requestFullscreen).not.toHaveBeenCalled();
  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(popup.resizeTo).not.toHaveBeenCalled();
});

it("falls back to popup only when permission is rejected", async () => {
  const popup = {
    moveTo: vi.fn(),
    resizeTo: vi.fn()
  };
  const openWindow = vi.fn(() => popup);
  const requestFullscreen = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: async () => {
      throw new Error("permission denied");
    },
    openWindow,
    requestFullscreen
  });

  expect(requestFullscreen).not.toHaveBeenCalled();
  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(popup.resizeTo).not.toHaveBeenCalled();
});

it("skips popup placement when the popup is blocked even after the synchronous open", async () => {
  const current: ScreenDetailed = {
    availLeft: 0,
    availTop: 0,
    availWidth: 1440,
    availHeight: 900
  };
  const other: ScreenDetailed = {
    availLeft: 1440,
    availTop: 0,
    availWidth: 1920,
    availHeight: 1080
  };
  const openWindow = vi.fn(() => null);
  const requestFullscreen = vi.fn(async () => undefined);

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    openWindow,
    requestFullscreen
  });

  expect(requestFullscreen).toHaveBeenCalledWith({ screen: other });
  expect(openWindow).toHaveBeenCalledTimes(1);
});
```

Implementation:

```ts
// packages/peitho-present/src/presentDisplay.ts
export type OpenPresenterWithDisplayOptions = {
  window?: Window;
  document?: Document;
  url?: string;
  getScreenDetails?: (() => Promise<ScreenDetails>) | undefined;
  openWindow?: (url: string, target: string, features: string) => PresenterPopup | null;
  requestFullscreen?: (options?: FullscreenOptions) => Promise<void> | void;
};

export type PresenterPopup = Pick<Window, "moveTo" | "resizeTo">;

export async function openPresenterWithDisplay(
  options: OpenPresenterWithDisplayOptions = {}
): Promise<PresenterPopup | null> {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const url = options.url ?? PRESENTER_URL;
  const openWindow =
    options.openWindow ??
    ((nextUrl, target, features) => win.open(nextUrl, target, features));
  const popup = openWindow(url, PRESENTER_TARGET, fallbackFeatures());
  const requestFullscreen =
    options.requestFullscreen ??
    ((fullscreenOptions?: FullscreenOptions) =>
      doc.documentElement.requestFullscreen?.(fullscreenOptions));
  const getScreenDetails = options.getScreenDetails ?? win.getScreenDetails?.bind(win);

  if (!getScreenDetails) return popup;

  try {
    const details = await getScreenDetails();
    const otherScreen = chooseOtherScreen(details);
    if (otherScreen) {
      await requestFullscreen({ screen: otherScreen });
      if (popup) {
        popup.moveTo(details.currentScreen.availLeft, details.currentScreen.availTop);
        popup.resizeTo(
          Math.min(DEFAULT_POPUP_WIDTH, details.currentScreen.availWidth),
          Math.min(DEFAULT_POPUP_HEIGHT, details.currentScreen.availHeight)
        );
      }
    }
  } catch {
    return popup;
  }

  return popup;
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- presentDisplay.test.ts
```

### Task 7 - Use Display Helper from Controls

Goal: the Presenter button default behavior opens a popup through `openPresenterWithDisplay`; explicit `openPresenter` injection still wins.

Files:

- `packages/peitho-present/src/controls.ts`
- `packages/peitho-present/test/controls.test.ts`

Test:

```ts
// packages/peitho-present/test/controls.test.ts
it("opens presenter popup with default display management", async () => {
  const root = document.createElement("main");
  const openWindow = vi.fn();
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    openPresenter: undefined,
    openPresenterWindow: openWindow,
    getScreenDetails: undefined
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="presenter"]')?.click();
  await Promise.resolve();

  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
});

it("keeps the explicit openPresenter injection as the highest priority", () => {
  const root = document.createElement("main");
  const openPresenter = vi.fn();
  const openPresenterWindow = vi.fn();
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    openPresenter,
    openPresenterWindow
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="presenter"]')?.click();

  expect(openPresenter).toHaveBeenCalledTimes(1);
  expect(openPresenterWindow).not.toHaveBeenCalled();
});
```

Implementation:

```ts
// packages/peitho-present/src/controls.ts
import {
  openPresenterWithDisplay,
  type OpenPresenterWithDisplayOptions
} from "./presentDisplay";

export type PresentationControlsOptions = {
  root: HTMLElement;
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  idleMs?: number;
  openPresenter?: () => void | Promise<void>;
  getScreenDetails?: OpenPresenterWithDisplayOptions["getScreenDetails"];
  openPresenterWindow?: OpenPresenterWithDisplayOptions["openWindow"];
  requestFullscreen?: OpenPresenterWithDisplayOptions["requestFullscreen"];
};

const openPresenter =
  options.openPresenter ??
  (() =>
    openPresenterWithDisplay({
      window: win,
      document: doc,
      getScreenDetails: options.getScreenDetails,
      openWindow: options.openPresenterWindow,
      requestFullscreen: options.requestFullscreen
    }));

// inside onClick
if (action === "presenter") void openPresenter();
```

Verification:

```sh
cd packages/peitho-present
npm test -- controls.test.ts
```

### Task 8 - Export Display Management API

Goal: `shell.js` bundle exposes display helpers for future presenter/display wiring tests, and typecheck sees the new declarations.

Files:

- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/generated.test.ts`

Test:

```ts
// packages/peitho-present/test/generated.test.ts
import {
  buildPresenterFeatures,
  chooseOtherScreen,
  openPresenterWithDisplay
} from "../src/index";

it("exports display management helpers", () => {
  expect(typeof buildPresenterFeatures).toBe("function");
  expect(typeof chooseOtherScreen).toBe("function");
  expect(typeof openPresenterWithDisplay).toBe("function");
});
```

Implementation:

```ts
// packages/peitho-present/src/index.ts
export {
  buildPresenterFeatures,
  chooseOtherScreen,
  fallbackFeatures,
  openPresenterWithDisplay,
  PRESENTER_URL
} from "./presentDisplay";
export type { OpenPresenterWithDisplayOptions } from "./presentDisplay";
```

Verification:

```sh
cd packages/peitho-present
npm test -- generated.test.ts
npm run build
rg "openPresenterWithDisplay" dist/shell.js
```

### Task 9 - Verify present HTML Uses Control Defaults

Goal: `present.html` does not need new markup; it still installs controls with default display management before mounting the shell.

Files:

- `crates/peitho-core/src/render.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn present_index_keeps_controls_default_display_management_before_mount() {
    let html = render_present_index();

    let controls_index = html
        .find("installPresentationControls({ root, window, document })")
        .unwrap();
    let mount_index = html.find("await mountPresentShell({ root })").unwrap();
    assert!(controls_index < mount_index);
    assert!(html.contains("installPresentationControls({ root, window, document })"));
    assert!(!html.contains("openPresenter"));
}
```

Update the existing repository-root present smoke test so it remains independent:

```rust
// crates/peitho/tests/present.rs
#[test]
fn repository_example_present_no_serve_smoke() {
    let shell = workspace_root().join("packages/peitho-present/dist/shell.js");
    assert!(shell.exists(), "shell bundle not built; run npm run build");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "present",
            "examples/deck.md",
            "--template",
            "templates/title-body-code.html",
            "--base-css",
            "themes/base.css",
            "--overrides-css",
            "themes/overrides.css",
            "--no-serve",
            "--no-open",
        ])
        .assert()
        .success();

    let cache = workspace_root().join(".peitho/present-cache");
    let shell_js = fs::read_to_string(cache.join("shell.js")).unwrap();

    assert!(shell_js.contains("openPresenterWithDisplay"));
    assert!(shell_js.contains("getScreenDetails"));
    assert!(shell_js.contains("requestFullscreen"));
}
```

Implementation:

- No `render_present_index` string change is expected.
- Do not add another repository-root smoke test that writes `.peitho/present-cache/`; extend the existing one to avoid cache races.

Verification:

```sh
cargo test -p peitho-core render::tests::present_index_keeps_controls_default_display_management_before_mount
cargo test -p peitho --test present repository_example_present_no_serve_smoke
```

### Task 10 - Full Verification Gate

Goal: prove display management changes are complete across Rust, TypeScript, generated bundle, and CLI smoke behavior.

Files:

- No new files. This task runs the repository gates after Tasks 1-9.

Commands:

```sh
cd packages/peitho-present
npm run build
npm test
npm run typecheck
cd ../..
cargo test --workspace
git diff --exit-code bindings/
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p peitho -- present examples/deck.md --template templates/title-body-code.html --base-css themes/base.css --overrides-css themes/overrides.css --no-serve --no-open
```

Expected markers:

```sh
rg "openPresenterWithDisplay" packages/peitho-present/dist/shell.js
rg "getScreenDetails" packages/peitho-present/dist/shell.js
rg "requestFullscreen" packages/peitho-present/dist/shell.js
rg "installPresentationControls" .peitho/present-cache/present.html
rg "await mountPresentShell" .peitho/present-cache/present.html
```

Manual follow-up for Opus/browser verification:

- macOS with Google Chrome installed should open `open -na "Google Chrome" --args --app=<URL> --start-fullscreen`.
- macOS without Google Chrome should fall back to `open <URL>`.
- Linux with `google-chrome` or `chromium` in `PATH` should use app mode and `--start-fullscreen`.
- Linux without either should fall back to `xdg-open`.
- Presenter button should open a popup named `peitho-presenter`; on browsers with Window Management API and permission granted, slide fullscreen should target the non-current screen.
