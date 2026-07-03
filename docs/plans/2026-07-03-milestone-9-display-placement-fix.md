# Peitho Milestone 9 Display Placement Fix Plan

## Purpose

Milestone 9 fixes the two real-device failures found after Milestone 8:

- Chrome app mode opened, but fullscreen was not reliable when Chrome was already running. The fix is to launch Chrome-family browsers with a dedicated profile at `$HOME/.peitho/chrome-profile`, plus `--no-first-run` and `--no-default-browser-check`, so `--app` and `--start-fullscreen` are applied by a fresh Chrome process. The profile is under the user home directory because browser permissions and first-run state are machine-level state, not project build artifacts.
- Window placement failed after the first Window Management API permission prompt because `requestFullscreen({ screen })` also requires transient user activation. The fix is activation-aware: open the popup synchronously, request screen details, try placement, and if fullscreen fails with `NotAllowedError`, show a visible "Click to place windows / クリックで画面を配置" overlay. The overlay click creates a new user activation and retries the same placement function.

This milestone does not add CLI-side display enumeration, Firefox/Safari multi-screen support, or new command flags. Existing `--no-open`, `--no-serve`, `--port`, and `--shell` behavior remains.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `crates/peitho/src/browser.rs` | Chrome profile path planning, browser command planning, profile directory creation before spawn | `std::env`, `std::fs`, `std::process::Command` |
| `crates/peitho/src/lib.rs` | Existing `browser` module export remains | `browser.rs` |
| `packages/peitho-present/src/presentDisplay.ts` | Placement extraction, fullscreen retry overlay, activation-aware presenter opening | Window Management API declarations |
| `packages/peitho-present/src/controls.ts` | Pass optional placement overlay injection to display helper | `presentDisplay.ts` |
| `packages/peitho-present/src/index.ts` | Export placement helpers and overlay types for tests and future shell wiring | `presentDisplay.ts` |
| `packages/peitho-present/test/presentDisplay.test.ts` | Deterministic two-screen placement and overlay retry tests | `presentDisplay.ts` |
| `packages/peitho-present/test/controls.test.ts` | Control-bar default path can exercise overlay injection | `controls.ts` |
| `packages/peitho-present/test/generated.test.ts` | Public export smoke for new helpers | `index.ts` |

Dependency order:

1. Add Chrome profile path to browser command planning.
2. Make `open_browser` create the profile directory before app-mode launch and fall back when it cannot.
3. Extract TS placement into `placeWindows`.
4. Add the default DOM placement overlay.
5. Wire retry-on-`NotAllowedError` into `openPresenterWithDisplay`.
6. Thread overlay injection through controls and exports.
7. Run all Rust and TS gates and report the concrete command/overlay behavior.

## Implementation Tasks

### Task 1 - Add Chrome Profile Path to Browser Environment

Goal: make profile-dir availability part of pure browser command planning so app-mode Chrome commands always carry `--user-data-dir` when they use Chrome.

Files:

- `crates/peitho/src/browser.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn macos_chrome_uses_dedicated_profile_and_fullscreen_flags() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Macos,
        mac_google_chrome_available: true,
        linux_browser: None,
        chrome_profile_dir: Some(PathBuf::from("/Users/alice/.peitho/chrome-profile")),
    };

    let command = plan_browser_command("http://127.0.0.1:8000/present.html", &env).unwrap();

    assert_eq!(command.program, OsString::from("open"));
    assert_eq!(
        command.args,
        vec![
            OsString::from("-na"),
            OsString::from("Google Chrome"),
            OsString::from("--args"),
            OsString::from("--user-data-dir=/Users/alice/.peitho/chrome-profile"),
            OsString::from("--no-first-run"),
            OsString::from("--no-default-browser-check"),
            OsString::from("--app=http://127.0.0.1:8000/present.html"),
            OsString::from("--start-fullscreen"),
        ]
    );
}

#[test]
fn macos_chrome_without_profile_falls_back_to_plain_open() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Macos,
        mac_google_chrome_available: true,
        linux_browser: None,
        chrome_profile_dir: None,
    };

    let command = plan_browser_command("http://127.0.0.1:8000/present.html", &env).unwrap();

    assert_eq!(command.program, OsString::from("open"));
    assert_eq!(
        command.args,
        vec![OsString::from("http://127.0.0.1:8000/present.html")]
    );
}
```

Expected Red:

```text
error[E0063]: missing field `chrome_profile_dir` in initializer of `BrowserEnvironment`
```

Implementation:

```rust
// crates/peitho/src/browser.rs
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserEnvironment {
    pub platform: BrowserPlatform,
    pub mac_google_chrome_available: bool,
    pub linux_browser: Option<OsString>,
    pub chrome_profile_dir: Option<PathBuf>,
}

fn chrome_args(url: &str, profile_dir: &Path) -> Vec<OsString> {
    vec![
        OsString::from(format!("--user-data-dir={}", profile_dir.display())),
        OsString::from("--no-first-run"),
        OsString::from("--no-default-browser-check"),
        OsString::from(format!("--app={url}")),
        OsString::from("--start-fullscreen"),
    ]
}

pub fn plan_browser_command(url: &str, env: &BrowserEnvironment) -> Option<BrowserCommand> {
    match env.platform {
        BrowserPlatform::Macos
            if env.mac_google_chrome_available && env.chrome_profile_dir.is_some() =>
        {
            let profile_dir = env.chrome_profile_dir.as_deref().expect("guarded by is_some");
            let mut args = vec![
                OsString::from("-na"),
                OsString::from("Google Chrome"),
                OsString::from("--args"),
            ];
            args.extend(chrome_args(url, profile_dir));
            Some(BrowserCommand {
                program: OsString::from("open"),
                args,
            })
        }
        BrowserPlatform::Macos => Some(BrowserCommand {
            program: OsString::from("open"),
            args: vec![OsString::from(url)],
        }),
        BrowserPlatform::Linux => linux_browser_command(
            url,
            env.linux_browser.as_deref(),
            env.chrome_profile_dir.as_deref(),
        ),
        BrowserPlatform::Other => None,
    }
}
```

Verification:

```sh
cargo test -p peitho browser::tests::macos_chrome_uses_dedicated_profile_and_fullscreen_flags
cargo test -p peitho browser::tests::macos_chrome_without_profile_falls_back_to_plain_open
```

### Task 2 - Update Linux Chrome Planning for Dedicated Profile

Goal: Linux Chrome and Chromium commands include the profile and first-run suppression flags; no profile falls back to `xdg-open`.

Files:

- `crates/peitho/src/browser.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn linux_chrome_uses_profile_and_fullscreen_flags() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Linux,
        mac_google_chrome_available: false,
        linux_browser: Some(OsString::from("google-chrome")),
        chrome_profile_dir: Some(PathBuf::from("/home/alice/.peitho/chrome-profile")),
    };

    let command = plan_browser_command("http://127.0.0.1:9000/present.html", &env).unwrap();

    assert_eq!(command.program, OsString::from("google-chrome"));
    assert_eq!(
        command.args,
        vec![
            OsString::from("--user-data-dir=/home/alice/.peitho/chrome-profile"),
            OsString::from("--no-first-run"),
            OsString::from("--no-default-browser-check"),
            OsString::from("--app=http://127.0.0.1:9000/present.html"),
            OsString::from("--start-fullscreen"),
        ]
    );
}

#[test]
fn linux_chromium_uses_profile_and_fullscreen_flags() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Linux,
        mac_google_chrome_available: false,
        linux_browser: Some(OsString::from("chromium")),
        chrome_profile_dir: Some(PathBuf::from("/home/alice/.peitho/chrome-profile")),
    };

    let command = plan_browser_command("http://127.0.0.1:9000/present.html", &env).unwrap();

    assert_eq!(command.program, OsString::from("chromium"));
    assert_eq!(
        command.args,
        vec![
            OsString::from("--user-data-dir=/home/alice/.peitho/chrome-profile"),
            OsString::from("--no-first-run"),
            OsString::from("--no-default-browser-check"),
            OsString::from("--app=http://127.0.0.1:9000/present.html"),
            OsString::from("--start-fullscreen"),
        ]
    );
}

#[test]
fn linux_chrome_without_profile_falls_back_to_xdg_open() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Linux,
        mac_google_chrome_available: false,
        linux_browser: Some(OsString::from("google-chrome")),
        chrome_profile_dir: None,
    };

    let command = plan_browser_command("http://127.0.0.1:9000/present.html", &env).unwrap();

    assert_eq!(command.program, OsString::from("xdg-open"));
    assert_eq!(
        command.args,
        vec![OsString::from("http://127.0.0.1:9000/present.html")]
    );
}
```

Expected Red:

```text
assertion `left == right` failed
left: ["--app=http://127.0.0.1:9000/present.html", "--start-fullscreen"]
right: ["--user-data-dir=/home/alice/.peitho/chrome-profile", "--no-first-run", "--no-default-browser-check", "--app=http://127.0.0.1:9000/present.html", "--start-fullscreen"]
```

Implementation:

```rust
// crates/peitho/src/browser.rs
fn linux_browser_command(
    url: &str,
    browser: Option<&OsStr>,
    profile_dir: Option<&Path>,
) -> Option<BrowserCommand> {
    match (browser, profile_dir) {
        (Some(program), Some(profile_dir)) => Some(BrowserCommand {
            program: program.to_owned(),
            args: chrome_args(url, profile_dir),
        }),
        _ => Some(BrowserCommand {
            program: OsString::from("xdg-open"),
            args: vec![OsString::from(url)],
        }),
    }
}
```

Verification:

```sh
cargo test -p peitho browser::tests::linux_chrome_uses_profile_and_fullscreen_flags
cargo test -p peitho browser::tests::linux_chromium_uses_profile_and_fullscreen_flags
cargo test -p peitho browser::tests::linux_chrome_without_profile_falls_back_to_xdg_open
```

### Task 3 - Derive Profile Directory from HOME

Goal: `current_environment` derives `$HOME/.peitho/chrome-profile`, and missing `HOME` removes the Chrome profile path so command planning falls back.

Files:

- `crates/peitho/src/browser.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn chrome_profile_dir_uses_home_peitho_chrome_profile() {
    assert_eq!(
        chrome_profile_dir_from_home(Some(OsString::from("/Users/alice"))),
        Some(PathBuf::from("/Users/alice/.peitho/chrome-profile"))
    );
}

#[test]
fn chrome_profile_dir_is_absent_without_home() {
    assert_eq!(chrome_profile_dir_from_home(None), None);
}

#[test]
fn current_environment_sets_supported_platform_shape_and_profile_slot() {
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

Expected Red:

```text
error[E0425]: cannot find function `chrome_profile_dir_from_home` in this scope
```

Implementation:

```rust
// crates/peitho/src/browser.rs
fn chrome_profile_dir_from_home(home: Option<OsString>) -> Option<PathBuf> {
    home.map(PathBuf::from)
        .map(|home| home.join(".peitho").join("chrome-profile"))
}

fn current_environment() -> BrowserEnvironment {
    BrowserEnvironment {
        platform: current_platform(),
        mac_google_chrome_available: chrome_app_exists(),
        linux_browser: find_linux_browser(),
        chrome_profile_dir: chrome_profile_dir_from_home(std::env::var_os("HOME")),
    }
}
```

Verification:

```sh
cargo test -p peitho browser::tests::chrome_profile_dir_uses_home_peitho_chrome_profile
cargo test -p peitho browser::tests::chrome_profile_dir_is_absent_without_home
cargo test -p peitho browser::tests::current_environment_sets_supported_platform_shape_and_profile_slot
```

### Task 4 - Create Profile Directory Before Spawn and Fall Back on Failure

Goal: `open_browser` creates the Chrome profile directory before spawning; if creation fails, it warns and replans without the profile instead of launching Chrome without a dedicated profile.

Files:

- `crates/peitho/src/browser.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn prepare_profile_keeps_existing_profile_dir() {
    let temp = tempfile::tempdir().expect("temp dir");
    let profile = temp.path().join(".peitho").join("chrome-profile");

    assert!(prepare_profile_dir(Some(&profile)));
    assert!(profile.is_dir());
}

#[test]
fn prepare_profile_reports_failure_for_file_parent() {
    let temp = tempfile::tempdir().expect("temp dir");
    let file_parent = temp.path().join("not-a-dir");
    std::fs::write(&file_parent, "file").expect("write marker");
    let profile = file_parent.join("chrome-profile");

    assert!(!prepare_profile_dir(Some(&profile)));
}

#[test]
fn prepare_profile_is_false_without_profile_path() {
    assert!(!prepare_profile_dir(None));
}
```

Expected Red:

```text
error[E0425]: cannot find function `prepare_profile_dir` in this scope
```

Implementation:

```rust
// crates/peitho/src/browser.rs
fn prepare_profile_dir(profile_dir: Option<&Path>) -> bool {
    let Some(profile_dir) = profile_dir else {
        return false;
    };
    match std::fs::create_dir_all(profile_dir) {
        Ok(()) => true,
        Err(err) => {
            eprintln!(
                "warning: failed to prepare Chrome profile at {}: {err}",
                profile_dir.display()
            );
            false
        }
    }
}

pub fn open_browser(url: &str) {
    let mut env = current_environment();
    if !prepare_profile_dir(env.chrome_profile_dir.as_deref()) {
        env.chrome_profile_dir = None;
    }

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
cargo test -p peitho browser::tests::prepare_profile_keeps_existing_profile_dir
cargo test -p peitho browser::tests::prepare_profile_reports_failure_for_file_parent
cargo test -p peitho browser::tests::prepare_profile_is_false_without_profile_path
```

### Task 5 - Extract TS Window Placement Function

Goal: use one `placeWindows` function for the first placement attempt and the overlay retry.

Files:

- `packages/peitho-present/src/presentDisplay.ts`
- `packages/peitho-present/test/presentDisplay.test.ts`

Test:

```ts
// packages/peitho-present/test/presentDisplay.test.ts
import { placeWindows } from "../src/presentDisplay";

it("places the slide fullscreen on the other screen and popup on the current screen", async () => {
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
  const requestFullscreen = vi.fn(async () => undefined);

  await placeWindows({
    details: { currentScreen: current, screens: [current, other] },
    popup,
    requestFullscreen
  });

  expect(requestFullscreen).toHaveBeenCalledWith({ screen: other });
  expect(popup.moveTo).toHaveBeenCalledWith(0, 0);
  expect(popup.resizeTo).toHaveBeenCalledWith(1200, 800);
});

it("does nothing when there is no other screen", async () => {
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
  const requestFullscreen = vi.fn();

  await placeWindows({
    details: { currentScreen: current, screens: [current] },
    popup,
    requestFullscreen
  });

  expect(requestFullscreen).not.toHaveBeenCalled();
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(popup.resizeTo).not.toHaveBeenCalled();
});
```

Expected Red:

```text
Module '"../src/presentDisplay"' has no exported member 'placeWindows'
```

Implementation:

```ts
// packages/peitho-present/src/presentDisplay.ts
export type RequestFullscreen = (options?: FullscreenOptions) => Promise<void> | void;

export type PlaceWindowsOptions = {
  details: ScreenDetails;
  popup: PresenterPopup | null;
  requestFullscreen: RequestFullscreen;
};

export async function placeWindows(options: PlaceWindowsOptions): Promise<boolean> {
  const otherScreen = chooseOtherScreen(options.details);
  if (!otherScreen) return false;

  await options.requestFullscreen({ screen: otherScreen });

  if (options.popup) {
    options.popup.moveTo(
      options.details.currentScreen.availLeft,
      options.details.currentScreen.availTop
    );
    options.popup.resizeTo(
      Math.min(DEFAULT_POPUP_WIDTH, options.details.currentScreen.availWidth),
      Math.min(DEFAULT_POPUP_HEIGHT, options.details.currentScreen.availHeight)
    );
  }

  return true;
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- presentDisplay.test.ts
npm run typecheck
```

### Task 6 - Add Default Placement Overlay

Goal: the slide window can show a visible retry target when fullscreen placement needs a second user activation.

Files:

- `packages/peitho-present/src/presentDisplay.ts`
- `packages/peitho-present/test/presentDisplay.test.ts`

Test:

```ts
// packages/peitho-present/test/presentDisplay.test.ts
import { showPlacementOverlay } from "../src/presentDisplay";

it("shows a visible placement overlay and invokes the retry on click", async () => {
  const retry = vi.fn(async () => undefined);
  const overlay = showPlacementOverlay(document, retry);
  const button = document.querySelector<HTMLButtonElement>("[data-peitho-place-overlay]");

  expect(button).not.toBeNull();
  expect(button?.textContent).toContain("Click to place windows");
  expect(button?.textContent).toContain("クリックで画面を配置");

  button?.click();
  await Promise.resolve();

  expect(retry).toHaveBeenCalledTimes(1);
  overlay.remove();
  expect(document.querySelector("[data-peitho-place-overlay]")).toBeNull();
});
```

Expected Red:

```text
Module '"../src/presentDisplay"' has no exported member 'showPlacementOverlay'
```

Implementation:

```ts
// packages/peitho-present/src/presentDisplay.ts
export type PlacementOverlay = {
  remove: () => void;
};

export type ShowPlacementOverlay = (retry: () => Promise<void>) => PlacementOverlay;

export function showPlacementOverlay(
  doc: Document,
  retry: () => Promise<void>
): PlacementOverlay {
  const button = doc.createElement("button");
  button.type = "button";
  button.dataset.peithoPlaceOverlay = "true";
  button.textContent = "Click to place windows / クリックで画面を配置";
  button.style.position = "fixed";
  button.style.inset = "0";
  button.style.zIndex = "2147483647";
  button.style.display = "grid";
  button.style.placeItems = "center";
  button.style.border = "0";
  button.style.background = "rgba(0, 0, 0, 0.82)";
  button.style.color = "#fff";
  button.style.font = "600 28px system-ui, sans-serif";
  button.addEventListener("click", () => {
    void retry();
  });
  doc.body.appendChild(button);

  return {
    remove: () => button.remove()
  };
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- presentDisplay.test.ts
npm run typecheck
```

### Task 7 - Retry Placement from Overlay After NotAllowedError

Goal: when the first `requestFullscreen({ screen })` fails after permission prompt activation loss, show the overlay and retry placement on overlay click. Permission denial from `getScreenDetails` remains a popup-only fallback.

Files:

- `packages/peitho-present/src/presentDisplay.ts`
- `packages/peitho-present/test/presentDisplay.test.ts`

Test:

```ts
// packages/peitho-present/test/presentDisplay.test.ts
it("shows overlay after NotAllowedError and retries placement from the overlay click", async () => {
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
  const requestFullscreen = vi.fn(async (_options?: FullscreenOptions) => undefined);
  requestFullscreen
    .mockRejectedValueOnce(new DOMException("activation expired", "NotAllowedError"))
    .mockResolvedValueOnce(undefined);
  let retry: (() => Promise<void>) | null = null;
  const overlay = { remove: vi.fn() };
  const showPlacementOverlay = vi.fn((nextRetry: () => Promise<void>) => {
    retry = nextRetry;
    return overlay;
  });

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    openWindow: vi.fn(() => popup),
    requestFullscreen,
    showPlacementOverlay
  });

  expect(showPlacementOverlay).toHaveBeenCalledTimes(1);
  expect(popup.moveTo).not.toHaveBeenCalled();
  expect(retry).not.toBeNull();

  await retry?.();

  expect(requestFullscreen).toHaveBeenNthCalledWith(2, { screen: other });
  expect(popup.moveTo).toHaveBeenCalledWith(0, 0);
  expect(popup.resizeTo).toHaveBeenCalledWith(1200, 800);
  expect(overlay.remove).toHaveBeenCalledTimes(1);
});

it("does not show overlay when the first placement succeeds", async () => {
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
  const showPlacementOverlay = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    openWindow: vi.fn(() => popup),
    requestFullscreen: vi.fn(async () => undefined),
    showPlacementOverlay
  });

  expect(showPlacementOverlay).not.toHaveBeenCalled();
  expect(popup.moveTo).toHaveBeenCalledWith(0, 0);
});

it("does not show overlay when popup is blocked", async () => {
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
  const showPlacementOverlay = vi.fn();

  await openPresenterWithDisplay({
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    openWindow: vi.fn(() => null),
    requestFullscreen: vi.fn(async () => {
      throw new DOMException("activation expired", "NotAllowedError");
    }),
    showPlacementOverlay
  });

  expect(showPlacementOverlay).not.toHaveBeenCalled();
});
```

Expected Red:

```text
Object literal may only specify known properties, and 'showPlacementOverlay' does not exist
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
  requestFullscreen?: RequestFullscreen;
  showPlacementOverlay?: ShowPlacementOverlay;
};

function isNotAllowedError(error: unknown): boolean {
  return error instanceof DOMException && error.name === "NotAllowedError";
}

export async function openPresenterWithDisplay(
  options: OpenPresenterWithDisplayOptions = {}
): Promise<PresenterPopup | null> {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const url = options.url ?? PRESENTER_URL;
  const openWindow =
    options.openWindow ?? ((nextUrl, target, features) => win.open(nextUrl, target, features));
  const popup = openWindow(url, PRESENTER_TARGET, fallbackFeatures());
  const requestFullscreen =
    options.requestFullscreen ??
    ((fullscreenOptions?: FullscreenOptions) =>
      doc.documentElement.requestFullscreen?.(fullscreenOptions));
  const getScreenDetails = options.getScreenDetails ?? win.getScreenDetails?.bind(win);
  const showOverlay =
    options.showPlacementOverlay ?? ((retry: () => Promise<void>) => showPlacementOverlay(doc, retry));

  if (!getScreenDetails) return popup;

  let details: ScreenDetails;
  try {
    details = await getScreenDetails();
  } catch {
    return popup;
  }

  try {
    await placeWindows({ details, popup, requestFullscreen });
  } catch (error) {
    if (!popup || !isNotAllowedError(error)) return popup;
    let overlay: PlacementOverlay | null = null;
    overlay = showOverlay(async () => {
      try {
        await placeWindows({ details, popup, requestFullscreen });
        overlay?.remove();
        overlay = null;
      } catch {
        return;
      }
    });
  }

  return popup;
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- presentDisplay.test.ts
npm run typecheck
```

### Task 8 - Thread Overlay Injection Through Controls

Goal: tests and future UI wiring can inject the placement overlay hook through `installPresentationControls`; default browser behavior still uses the DOM overlay.

Files:

- `packages/peitho-present/src/controls.ts`
- `packages/peitho-present/test/controls.test.ts`

Test:

```ts
// packages/peitho-present/test/controls.test.ts
it("passes the placement overlay hook to the default presenter display helper", async () => {
  const root = document.createElement("main");
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
  const showPlacementOverlay = vi.fn((retry: () => Promise<void>) => {
    void retry;
    return { remove: vi.fn() };
  });
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    openPresenterWindow: vi.fn(() => popup),
    getScreenDetails: async () => ({ currentScreen: current, screens: [current, other] }),
    requestFullscreen: vi.fn(async () => {
      throw new DOMException("activation expired", "NotAllowedError");
    }),
    showPlacementOverlay
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="presenter"]')?.click();
  await Promise.resolve();
  await Promise.resolve();

  expect(showPlacementOverlay).toHaveBeenCalledTimes(1);
});
```

Expected Red:

```text
Object literal may only specify known properties, and 'showPlacementOverlay' does not exist
```

Implementation:

```ts
// packages/peitho-present/src/controls.ts
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
  showPlacementOverlay?: OpenPresenterWithDisplayOptions["showPlacementOverlay"];
};

const openPresenter =
  options.openPresenter ??
  (() =>
    openPresenterWithDisplay({
      window: win,
      document: doc,
      getScreenDetails: options.getScreenDetails,
      openWindow: options.openPresenterWindow,
      requestFullscreen: options.requestFullscreen,
      showPlacementOverlay: options.showPlacementOverlay
    }));
```

Verification:

```sh
cd packages/peitho-present
npm test -- controls.test.ts
npm run typecheck
```

### Task 9 - Export New Placement Helpers

Goal: public package exports include the placement helper and overlay types used by tests and future shell wiring.

Files:

- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/generated.test.ts`

Test:

```ts
// packages/peitho-present/test/generated.test.ts
import {
  openPresenterWithDisplay,
  placeWindows,
  showPlacementOverlay
} from "../src/index";
import type {
  PlacementOverlay,
  PlaceWindowsOptions,
  RequestFullscreen,
  ShowPlacementOverlay
} from "../src/index";

it("exports display placement retry helpers", () => {
  const overlay: PlacementOverlay = { remove: () => undefined };
  const fullscreen: RequestFullscreen = () => undefined;
  const show: ShowPlacementOverlay = () => overlay;
  const options: PlaceWindowsOptions = {
    details: {
      currentScreen: { availLeft: 0, availTop: 0, availWidth: 1, availHeight: 1 },
      screens: [{ availLeft: 0, availTop: 0, availWidth: 1, availHeight: 1 }]
    },
    popup: null,
    requestFullscreen: fullscreen
  };

  expect(openPresenterWithDisplay).toBeTypeOf("function");
  expect(placeWindows).toBeTypeOf("function");
  expect(showPlacementOverlay).toBeTypeOf("function");
  expect(show).toBeTypeOf("function");
  expect(options.popup).toBeNull();
});
```

Expected Red:

```text
Module '"../src/index"' has no exported member 'placeWindows'
```

Implementation:

```ts
// packages/peitho-present/src/index.ts
export {
  buildPresenterFeatures,
  chooseOtherScreen,
  fallbackFeatures,
  openPresenterWithDisplay,
  placeWindows,
  PRESENTER_URL,
  showPlacementOverlay
} from "./presentDisplay";
export type {
  OpenPresenterWithDisplayOptions,
  PlacementOverlay,
  PlaceWindowsOptions,
  PresenterPopup,
  RequestFullscreen,
  ShowPlacementOverlay
} from "./presentDisplay";
```

Verification:

```sh
cd packages/peitho-present
npm test -- generated.test.ts
npm run typecheck
```

### Task 10 - Full Gate and Real-Device Regression Markers

Goal: verify the implementation with the full project gates and report the two behavioral markers needed for real-device testing.

Files:

- `crates/peitho/src/browser.rs`
- `packages/peitho-present/src/presentDisplay.ts`
- `packages/peitho-present/dist/shell.js`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn macos_command_report_matches_author_expected_command() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Macos,
        mac_google_chrome_available: true,
        linux_browser: None,
        chrome_profile_dir: Some(PathBuf::from("/Users/alice/.peitho/chrome-profile")),
    };

    let command = plan_browser_command("http://127.0.0.1:8000/present.html", &env).unwrap();
    let rendered = std::iter::once(command.program.to_string_lossy().to_string())
        .chain(command.args.iter().map(|arg| arg.to_string_lossy().to_string()))
        .collect::<Vec<_>>()
        .join(" ");

    assert_eq!(
        rendered,
        "open -na Google Chrome --args --user-data-dir=/Users/alice/.peitho/chrome-profile --no-first-run --no-default-browser-check --app=http://127.0.0.1:8000/present.html --start-fullscreen"
    );
}
```

Implementation:

- Keep the `macos_command_report_matches_author_expected_command` test in `browser.rs`.
- Run `npm run build` after TS changes so `packages/peitho-present/dist/shell.js` contains `data-peitho-place-overlay`, `NotAllowedError`, and `placeWindows`.
- Do not commit `dist/`; it remains a generated artifact for local verification and existing present tests.

Verification:

```sh
cd packages/peitho-present
npm run build
npm test
npm run typecheck

cd ../..
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cargo run -p peitho -- present examples/deck.md --template templates/title-body-code.html --base-css themes/base.css --overrides-css themes/overrides.css --no-serve --no-open
rg -n "user-data-dir|no-first-run|no-default-browser-check|start-fullscreen" crates/peitho/src/browser.rs
rg -n "data-peitho-place-overlay|NotAllowedError|placeWindows" packages/peitho-present/dist/shell.js
```

Report these concrete values:

```text
Mac app-mode command:
open -na "Google Chrome" --args --user-data-dir=$HOME/.peitho/chrome-profile --no-first-run --no-default-browser-check --app=<URL> --start-fullscreen

Overlay marker:
data-peitho-place-overlay is present in shell.js and is only shown after requestFullscreen fails with NotAllowedError while a popup exists.
```

## Summary

This plan has 10 TDD tasks. The Rust half first makes Chrome app-mode launch deterministic by requiring a home-scoped Peitho Chrome profile, then creating that profile before spawning and falling back if it cannot be prepared. The TypeScript half extracts window placement, adds a visible activation-retry overlay, wires it through the presenter control button, exports the helpers, and finishes with full Rust/TS gates plus the exact command and overlay markers needed for real-device verification.
