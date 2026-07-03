# Peitho Milestone 10 CLI Window Placement Plan

## Purpose

Milestone 10 moves display placement out of browser JavaScript and into the `peitho present` CLI.

Measured facts now override the Milestone 8/9 design:

- macOS app-mode fullscreen Spaces conflict with Window Management API placement. Presenter popups can be captured into the same fullscreen Space, so JS placement is unreliable.
- Chrome ignores the second window's `--window-position` / `--window-size` when both launches hand off to the same running profile.
- Two fresh Chrome-family instances with separate `--user-data-dir` values do honor `--window-position`, `--window-size`, and `--start-fullscreen`.
- CLI app windows can close themselves via `window.close()`.
- macOS display frames are available through JXA and `NSScreen`, but `NSScreen` uses a bottom-left origin. Chrome wants global coordinates in the primary screen's top-left coordinate space, so `chrome_y = primary_height - (nsscreen_y + height)`.

Therefore M10 deliberately removes the Window Management API path. The CLI owns OS-level window placement, and cross-profile slide synchronization moves from `BroadcastChannel` to a local HTTP transport. The shell's DOM event contract remains unchanged: UI emits `peitho:navigate`, shell emits `peitho:slidechange`, and sync bridges only translate between DOM events and a transport.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `crates/peitho/Cargo.toml` | Add `serde` to `peitho` crate dependencies if not already direct | workspace `serde` |
| `crates/peitho/src/displays.rs` | macOS JXA display detection, `NSScreen` JSON parsing, Chrome coordinate conversion, pure layout planning | `serde`, `serde_json`, `std::process::Command` |
| `crates/peitho/src/browser.rs` | Single-window and two-window browser command planning, split Chrome profiles, profile preparation, process spawning | `displays.rs`, `std::process::Command` |
| `crates/peitho/src/lib.rs` | Export `displays` alongside `browser` and `server` | local modules |
| `crates/peitho/src/main.rs` | Add `present --no-presenter`, compute presenter URL, detect display layout, call browser planner | `browser.rs`, `displays.rs` |
| `crates/peitho/src/server.rs` | Add `/sync` SSE subscription and `POST /sync` relay while preserving static file serving | `tiny_http`, `serde`, `serde_json`, `std::sync::mpsc` |
| `crates/peitho/tests/present.rs` | Present cache and server sync integration tests | CLI binary, `server.rs` |
| `crates/peitho-core/src/render.rs` | Generated `present.html` / `presenter.html` use server sync factory and close controls | TS public API |
| `packages/peitho-present/src/sync.ts` | Add server sync `SyncChannelFactory` using `EventSource('/sync')` and `fetch('/sync', POST)` | DOM `EventSource`, `fetch` |
| `packages/peitho-present/src/presentDisplay.ts` | Remove Window Management API placement; keep popup-only helper or remove unused helpers | `window.open` |
| `packages/peitho-present/src/window-management.d.ts` | Delete; no Window Management API types remain | none |
| `packages/peitho-present/src/controls.ts` | Remove placement injection options; add Close control invoking injectable `closeWindow` | `window.close` |
| `packages/peitho-present/src/presenter.ts` | Add presenter Close button and server sync factory option remains | `sync.ts` |
| `packages/peitho-present/src/index.ts` | Export server sync factory; remove Window Management API exports | local TS modules |
| `packages/peitho-present/test/sync.test.ts` | Server sync channel tests with injected EventSource/fetch | `sync.ts` |
| `packages/peitho-present/test/controls.test.ts` | Close button and simplified Presenter button tests | `controls.ts` |
| `packages/peitho-present/test/presenter.test.ts` | Presenter close button test | `presenter.ts` |
| `packages/peitho-present/test/generated.test.ts` | Public export expectations updated | `index.ts` |

Dependency order:

1. Add display parsing and layout planning as pure Rust functions.
2. Extend browser command planning around split profiles and optional two-window layout.
3. Wire `present --no-presenter` and display detection into CLI launch.
4. Add server sync relay to `PresentServer`.
5. Add TS server sync channel and generated HTML wiring.
6. Remove Window Management API placement path and tests.
7. Add close controls.
8. Update cache/render smoke tests and run all gates.

## Implementation Tasks

### Task 1 - Add macOS Display Frame Parsing and Chrome Coordinate Conversion

Goal: parse JXA `NSScreen` JSON and convert bottom-left-origin screen frames into Chrome top-left-origin coordinates.

Files:

- `crates/peitho/Cargo.toml`
- `crates/peitho/src/displays.rs`
- `crates/peitho/src/lib.rs`

Test:

```rust
// crates/peitho/src/displays.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_nsscreen_frames_to_chrome_coordinates() {
        let json = r#"[
          {"x":0,"y":0,"width":1512,"height":982},
          {"x":-1055,"y":316,"width":1055,"height":666}
        ]"#;

        let displays = parse_nsscreen_json(json).unwrap();

        assert_eq!(
            displays,
            vec![
                ChromeDisplay {
                    x: 0,
                    y: 0,
                    width: 1512,
                    height: 982,
                    primary: true,
                },
                ChromeDisplay {
                    x: -1055,
                    y: 0,
                    width: 1055,
                    height: 666,
                    primary: false,
                },
            ]
        );
    }
}
```

Expected Red:

```text
file not found for module `displays`
```

Implementation:

```rust
// crates/peitho/src/lib.rs
pub mod browser;
pub mod displays;
pub mod server;
```

```toml
# crates/peitho/Cargo.toml
[dependencies]
serde.workspace = true
```

```rust
// crates/peitho/src/displays.rs
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeDisplay {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Debug, Deserialize)]
struct NsscreenFrame {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub fn parse_nsscreen_json(json: &str) -> Result<Vec<ChromeDisplay>, serde_json::Error> {
    let frames: Vec<NsscreenFrame> = serde_json::from_str(json)?;
    Ok(convert_nsscreen_frames(&frames))
}

fn convert_nsscreen_frames(frames: &[NsscreenFrame]) -> Vec<ChromeDisplay> {
    let primary = frames
        .iter()
        .find(|frame| frame.x == 0 && frame.y == 0)
        .or_else(|| frames.first());
    let Some(primary) = primary else {
        return Vec::new();
    };
    let primary_height = primary.height as i32;

    frames
        .iter()
        .map(|frame| ChromeDisplay {
            x: frame.x,
            y: primary_height - (frame.y + frame.height as i32),
            width: frame.width,
            height: frame.height,
            primary: frame.x == primary.x && frame.y == primary.y,
        })
        .collect()
}
```

Verification:

```sh
cargo test -p peitho displays::tests::converts_nsscreen_frames_to_chrome_coordinates
```

### Task 2 - Add Pure Presentation Layout Planning

Goal: choose slides on the first non-primary display and presenter on the primary display, with presenter size clamped to the primary display.

Files:

- `crates/peitho/src/displays.rs`

Test:

```rust
// crates/peitho/src/displays.rs
#[test]
fn plans_slides_on_external_and_presenter_on_primary() {
    let displays = vec![
        ChromeDisplay { x: 0, y: 0, width: 1512, height: 982, primary: true },
        ChromeDisplay { x: -1055, y: 0, width: 1055, height: 666, primary: false },
    ];

    let layout = plan_presentation_layout(&displays).unwrap();

    assert_eq!(
        layout.slides,
        WindowPlacement {
            x: -1055,
            y: 0,
            width: 1055,
            height: 666,
            fullscreen: true,
        }
    );
    assert_eq!(
        layout.presenter,
        WindowPlacement {
            x: 156,
            y: 91,
            width: 1200,
            height: 800,
            fullscreen: false,
        }
    );
}

#[test]
fn clamps_presenter_to_small_primary_display() {
    let displays = vec![
        ChromeDisplay { x: 0, y: 0, width: 900, height: 700, primary: true },
        ChromeDisplay { x: 900, y: 0, width: 1280, height: 720, primary: false },
    ];

    let layout = plan_presentation_layout(&displays).unwrap();

    assert_eq!(
        layout.presenter,
        WindowPlacement {
            x: 0,
            y: 0,
            width: 900,
            height: 700,
            fullscreen: false,
        }
    );
}

#[test]
fn returns_none_for_single_display() {
    let displays = vec![ChromeDisplay { x: 0, y: 0, width: 1512, height: 982, primary: true }];

    assert_eq!(plan_presentation_layout(&displays), None);
}
```

Expected Red:

```text
cannot find function `plan_presentation_layout` in this scope
```

Implementation:

```rust
// crates/peitho/src/displays.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationLayout {
    pub slides: WindowPlacement,
    pub presenter: WindowPlacement,
}

pub fn plan_presentation_layout(displays: &[ChromeDisplay]) -> Option<PresentationLayout> {
    let primary = displays.iter().find(|display| display.primary)?;
    let slides = displays.iter().find(|display| !display.primary)?;

    let presenter_width = 1200_u32.min(primary.width);
    let presenter_height = 800_u32.min(primary.height);
    let presenter_x = primary.x + ((primary.width - presenter_width) / 2) as i32;
    let presenter_y = primary.y + ((primary.height - presenter_height) / 2) as i32;

    Some(PresentationLayout {
        slides: WindowPlacement {
            x: slides.x,
            y: slides.y,
            width: slides.width,
            height: slides.height,
            fullscreen: true,
        },
        presenter: WindowPlacement {
            x: presenter_x,
            y: presenter_y,
            width: presenter_width,
            height: presenter_height,
            fullscreen: false,
        },
    })
}
```

Verification:

```sh
cargo test -p peitho displays::tests::plans_slides_on_external_and_presenter_on_primary
cargo test -p peitho displays::tests::clamps_presenter_to_small_primary_display
cargo test -p peitho displays::tests::returns_none_for_single_display
```

### Task 3 - Add macOS JXA Display Detection Wrapper

Goal: provide a fallible macOS-only runtime detector while keeping parsing and layout pure.

Files:

- `crates/peitho/src/displays.rs`

Test:

```rust
// crates/peitho/src/displays.rs
#[test]
fn jxa_script_mentions_appkit_nsscreen_and_json() {
    assert!(MACOS_DISPLAY_JXA.contains("ObjC.import('AppKit')"));
    assert!(MACOS_DISPLAY_JXA.contains("$.NSScreen.screens"));
    assert!(MACOS_DISPLAY_JXA.contains("JSON.stringify"));
}

#[test]
fn layout_from_jxa_output_returns_none_for_invalid_json() {
    assert_eq!(layout_from_jxa_output("not json"), None);
}

#[test]
fn layout_from_jxa_output_plans_valid_two_display_json() {
    let json = r#"[{"x":0,"y":0,"width":1512,"height":982},{"x":-1055,"y":316,"width":1055,"height":666}]"#;

    assert_eq!(
        layout_from_jxa_output(json).unwrap().slides,
        WindowPlacement { x: -1055, y: 0, width: 1055, height: 666, fullscreen: true }
    );
}
```

Expected Red:

```text
cannot find value `MACOS_DISPLAY_JXA` in this scope
```

Implementation:

```rust
// crates/peitho/src/displays.rs
use std::process::Command;

pub const MACOS_DISPLAY_JXA: &str = r#"
ObjC.import('AppKit');
const screens = $.NSScreen.screens;
const out = [];
for (let i = 0; i < screens.count; i++) {
  const frame = screens.objectAtIndex(i).frame;
  out.push({
    x: Math.round(frame.origin.x),
    y: Math.round(frame.origin.y),
    width: Math.round(frame.size.width),
    height: Math.round(frame.size.height)
  });
}
JSON.stringify(out);
"#;

pub fn layout_from_jxa_output(stdout: &str) -> Option<PresentationLayout> {
    let displays = parse_nsscreen_json(stdout).ok()?;
    plan_presentation_layout(&displays)
}

pub fn detect_presentation_layout() -> Option<PresentationLayout> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", MACOS_DISPLAY_JXA])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    layout_from_jxa_output(&stdout)
}
```

Verification:

```sh
cargo test -p peitho displays::tests::jxa_script_mentions_appkit_nsscreen_and_json
cargo test -p peitho displays::tests::layout_from_jxa_output_
```

### Task 4 - Plan Split Chrome Profiles and Single-Window Fallback

Goal: replace one profile with distinct slides/presenter profiles and keep one-window behavior for one display or `--no-presenter`.

Files:

- `crates/peitho/src/browser.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn chrome_profiles_are_split_by_window_role() {
    assert_eq!(
        chrome_profiles_from_home(Some(OsString::from("/Users/alice"))),
        Some(ChromeProfiles {
            slides: PathBuf::from("/Users/alice/.peitho/chrome-profile-slides"),
            presenter: PathBuf::from("/Users/alice/.peitho/chrome-profile-presenter"),
        })
    );
}

#[test]
fn macos_single_window_uses_slides_profile() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Macos,
        mac_google_chrome_available: true,
        linux_browser: None,
        chrome_profiles: Some(ChromeProfiles {
            slides: PathBuf::from("/Users/alice/.peitho/chrome-profile-slides"),
            presenter: PathBuf::from("/Users/alice/.peitho/chrome-profile-presenter"),
        }),
        layout: None,
    };

    let commands = plan_browser_commands(
        &BrowserOpenRequest {
            slides_url: "http://127.0.0.1:8000/present.html",
            presenter_url: "http://127.0.0.1:8000/presenter.html",
            no_presenter: false,
        },
        &env,
    );

    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].program, OsString::from("open"));
    assert_eq!(
        commands[0].args,
        vec![
            OsString::from("-na"),
            OsString::from("Google Chrome"),
            OsString::from("--args"),
            OsString::from("--user-data-dir=/Users/alice/.peitho/chrome-profile-slides"),
            OsString::from("--no-first-run"),
            OsString::from("--no-default-browser-check"),
            OsString::from("--app=http://127.0.0.1:8000/present.html"),
            OsString::from("--start-fullscreen"),
        ]
    );
}
```

Expected Red:

```text
cannot find function `plan_browser_commands` in this scope
```

Implementation:

```rust
// crates/peitho/src/browser.rs
use crate::displays::PresentationLayout;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromeProfiles {
    pub slides: PathBuf,
    pub presenter: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserEnvironment {
    pub platform: BrowserPlatform,
    pub mac_google_chrome_available: bool,
    pub linux_browser: Option<OsString>,
    pub chrome_profiles: Option<ChromeProfiles>,
    pub layout: Option<PresentationLayout>,
}

#[derive(Debug, Clone, Copy)]
pub struct BrowserOpenRequest<'a> {
    pub slides_url: &'a str,
    pub presenter_url: &'a str,
    pub no_presenter: bool,
}

pub fn chrome_profiles_from_home(home: Option<OsString>) -> Option<ChromeProfiles> {
    let root = home.map(PathBuf::from)?.join(".peitho");
    Some(ChromeProfiles {
        slides: root.join("chrome-profile-slides"),
        presenter: root.join("chrome-profile-presenter"),
    })
}

fn chrome_base_args(profile_dir: &Path, url: &str) -> Vec<OsString> {
    vec![
        OsString::from(format!("--user-data-dir={}", profile_dir.display())),
        OsString::from("--no-first-run"),
        OsString::from("--no-default-browser-check"),
        OsString::from(format!("--app={url}")),
    ]
}

fn chrome_slides_args(profile_dir: &Path, url: &str) -> Vec<OsString> {
    let mut args = chrome_base_args(profile_dir, url);
    args.push(OsString::from("--start-fullscreen"));
    args
}

fn macos_chrome_command(args: Vec<OsString>) -> BrowserCommand {
    let mut full_args = vec![
        OsString::from("-na"),
        OsString::from("Google Chrome"),
        OsString::from("--args"),
    ];
    full_args.extend(args);
    BrowserCommand {
        program: OsString::from("open"),
        args: full_args,
    }
}

pub fn plan_browser_commands(
    request: &BrowserOpenRequest<'_>,
    env: &BrowserEnvironment,
) -> Vec<BrowserCommand> {
    match env.platform {
        BrowserPlatform::Macos if env.mac_google_chrome_available => {
            let Some(profiles) = env.chrome_profiles.as_ref() else {
                return vec![BrowserCommand {
                    program: OsString::from("open"),
                    args: vec![OsString::from(request.slides_url)],
                }];
            };
            vec![macos_chrome_command(chrome_slides_args(
                &profiles.slides,
                request.slides_url,
            ))]
        }
        BrowserPlatform::Macos => vec![BrowserCommand {
            program: OsString::from("open"),
            args: vec![OsString::from(request.slides_url)],
        }],
        BrowserPlatform::Linux => linux_browser_commands(request, env),
        BrowserPlatform::Other => Vec::new(),
    }
}
```

Verification:

```sh
cargo test -p peitho browser::tests::chrome_profiles_are_split_by_window_role
cargo test -p peitho browser::tests::macos_single_window_uses_slides_profile
```

### Task 5 - Plan Two-Window Chrome Commands and `--no-presenter`

Goal: with a two-display layout, plan slides and presenter commands using separate profiles; `no_presenter` forces slides-only.

Files:

- `crates/peitho/src/browser.rs`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn macos_two_display_plan_launches_slides_then_presenter() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Macos,
        mac_google_chrome_available: true,
        linux_browser: None,
        chrome_profiles: Some(test_profiles()),
        layout: Some(test_layout()),
    };

    let commands = plan_browser_commands(&test_request(false), &env);

    assert_eq!(commands.len(), 2);
    assert_eq!(
        commands[0].args,
        vec![
            OsString::from("-na"),
            OsString::from("Google Chrome"),
            OsString::from("--args"),
            OsString::from("--user-data-dir=/Users/alice/.peitho/chrome-profile-slides"),
            OsString::from("--no-first-run"),
            OsString::from("--no-default-browser-check"),
            OsString::from("--app=http://127.0.0.1:8000/present.html"),
            OsString::from("--window-position=-1055,0"),
            OsString::from("--start-fullscreen"),
        ]
    );
    assert_eq!(
        commands[1].args,
        vec![
            OsString::from("-na"),
            OsString::from("Google Chrome"),
            OsString::from("--args"),
            OsString::from("--user-data-dir=/Users/alice/.peitho/chrome-profile-presenter"),
            OsString::from("--no-first-run"),
            OsString::from("--no-default-browser-check"),
            OsString::from("--app=http://127.0.0.1:8000/presenter.html"),
            OsString::from("--window-position=156,91"),
            OsString::from("--window-size=1200,800"),
        ]
    );
}

#[test]
fn no_presenter_forces_single_slides_window() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Macos,
        mac_google_chrome_available: true,
        linux_browser: None,
        chrome_profiles: Some(test_profiles()),
        layout: Some(test_layout()),
    };

    let commands = plan_browser_commands(&test_request(true), &env);

    assert_eq!(commands.len(), 1);
    assert!(commands[0].args.contains(&OsString::from("--start-fullscreen")));
    assert!(!commands[0].args.iter().any(|arg| arg == "--window-size=1200,800"));
}
```

Expected Red:

```text
assertion `left == right` failed
left: 1
right: 2
```

Implementation:

```rust
// crates/peitho/src/browser.rs
fn push_window_position(args: &mut Vec<OsString>, placement: crate::displays::WindowPlacement) {
    args.push(OsString::from(format!(
        "--window-position={},{}",
        placement.x, placement.y
    )));
}

fn push_window_size(args: &mut Vec<OsString>, placement: crate::displays::WindowPlacement) {
    args.push(OsString::from(format!(
        "--window-size={},{}",
        placement.width, placement.height
    )));
}

fn chrome_slides_args_for_layout(
    profile_dir: &Path,
    url: &str,
    placement: Option<crate::displays::WindowPlacement>,
) -> Vec<OsString> {
    let mut args = chrome_base_args(profile_dir, url);
    if let Some(placement) = placement {
        push_window_position(&mut args, placement);
    }
    args.push(OsString::from("--start-fullscreen"));
    args
}

fn chrome_presenter_args(
    profile_dir: &Path,
    url: &str,
    placement: crate::displays::WindowPlacement,
) -> Vec<OsString> {
    let mut args = chrome_base_args(profile_dir, url);
    push_window_position(&mut args, placement);
    push_window_size(&mut args, placement);
    args
}

// In the macOS Chrome branch of plan_browser_commands:
if let Some(layout) = env.layout.filter(|_| !request.no_presenter) {
    return vec![
        macos_chrome_command(chrome_slides_args_for_layout(
            &profiles.slides,
            request.slides_url,
            Some(layout.slides),
        )),
        macos_chrome_command(chrome_presenter_args(
            &profiles.presenter,
            request.presenter_url,
            layout.presenter,
        )),
    ];
}
vec![macos_chrome_command(chrome_slides_args_for_layout(
    &profiles.slides,
    request.slides_url,
    None,
))]
```

Verification:

```sh
cargo test -p peitho browser::tests::macos_two_display_plan_launches_slides_then_presenter
cargo test -p peitho browser::tests::no_presenter_forces_single_slides_window
```

### Task 6 - Wire `present --no-presenter` and Display Layout into CLI

Goal: CLI exposes `--no-presenter`, computes the presenter URL, detects layout, and passes all browser-open inputs through pure planning.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/src/browser.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
#[test]
fn present_help_lists_no_presenter_flag() {
    Command::cargo_bin("peitho")
        .unwrap()
        .args(["present", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--no-presenter"));
}

// crates/peitho/src/browser.rs
#[test]
fn presenter_url_uses_same_origin_as_present_url() {
    assert_eq!(
        presenter_url("http://127.0.0.1:49152/present.html"),
        "http://127.0.0.1:49152/presenter.html"
    );
}
```

Expected Red:

```text
Unexpected stdout, failed var.contains(--no-presenter)
```

Implementation:

```rust
// crates/peitho/src/main.rs
struct PresentOptions {
    input: PathBuf,
    template: PathBuf,
    base_css: PathBuf,
    overrides_css: PathBuf,
    shell: PathBuf,
    port: u16,
    no_open: bool,
    no_serve: bool,
    no_presenter: bool,
}

// In Command::Present:
#[arg(long)]
no_presenter: bool,

// In present():
let server = server::PresentServer::bind(cache, options.port)?;
let url = server.url();
let presenter_url = browser::presenter_url(&url);
println!("serving presentation at {url}");
std::io::stdout().flush().into_diagnostic()?;
if !options.no_open {
    let layout = displays::detect_presentation_layout();
    browser::open_browser(browser::BrowserOpenRequest {
        slides_url: &url,
        presenter_url: &presenter_url,
        no_presenter: options.no_presenter,
    }, layout);
}
```

```rust
// crates/peitho/src/browser.rs
pub fn presenter_url(slides_url: &str) -> String {
    slides_url.replace("/present.html", "/presenter.html")
}

pub fn open_browser(request: BrowserOpenRequest<'_>, layout: Option<PresentationLayout>) {
    let mut env = current_environment();
    env.layout = layout;
    if !prepare_profile_dirs(env.chrome_profiles.as_ref()) {
        env.chrome_profiles = None;
    }
    let commands = plan_browser_commands(&request, &env);
    if commands.is_empty() {
        eprintln!("warning: browser auto-open is not supported on this platform");
        return;
    }
    for command in commands {
        if let Err(err) = Command::new(&command.program).args(&command.args).spawn() {
            eprintln!(
                "warning: failed to open browser with {}: {err}",
                command.program.to_string_lossy()
            );
        }
    }
}
```

Verification:

```sh
cargo test -p peitho --test present present_help_lists_no_presenter_flag
cargo test -p peitho browser::tests::presenter_url_uses_same_origin_as_present_url
```

### Task 7 - Add SSE SyncHub and Streaming Response

Goal: add a concrete non-blocking SSE transport to `PresentServer` without blocking the accept loop.

Files:

- `crates/peitho/src/server.rs`

Test:

```rust
// crates/peitho/src/server.rs
#[test]
fn sync_hub_broadcasts_json_to_subscribers_and_drops_closed_clients() {
    let hub = SyncHub::default();
    let first = hub.subscribe();
    let second = hub.subscribe();
    drop(second);

    hub.broadcast(r#"{"index":2}"#);

    assert_eq!(first.recv_timeout(Duration::from_secs(1)).unwrap(), r#"{"index":2}"#);
    assert_eq!(hub.client_count(), 1);
}

#[test]
fn sse_stream_formats_data_events() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(r#"{"index":1}"#.to_owned()).unwrap();
    drop(tx);
    let mut stream = SseStream::new(rx);
    let mut text = String::new();

    stream.read_to_string(&mut text).unwrap();

    assert_eq!(text, "data: {\"index\":1}\n\n");
}
```

Expected Red:

```text
cannot find type `SyncHub` in this scope
```

Implementation:

```rust
// crates/peitho/src/server.rs
use std::{
    io::{Cursor, Read},
    sync::{mpsc, Arc, Mutex},
};

#[derive(Clone, Default)]
pub(crate) struct SyncHub {
    clients: Arc<Mutex<Vec<mpsc::Sender<String>>>>,
}

impl SyncHub {
    pub(crate) fn subscribe(&self) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        self.clients.lock().expect("sync hub mutex").push(tx);
        rx
    }

    pub(crate) fn broadcast(&self, message: &str) {
        let mut clients = self.clients.lock().expect("sync hub mutex");
        clients.retain(|client| client.send(message.to_owned()).is_ok());
    }

    #[cfg(test)]
    fn client_count(&self) -> usize {
        self.clients.lock().expect("sync hub mutex").len()
    }
}

struct SseStream {
    rx: mpsc::Receiver<String>,
    buffer: Cursor<Vec<u8>>,
}

impl SseStream {
    fn new(rx: mpsc::Receiver<String>) -> Self {
        Self {
            rx,
            buffer: Cursor::new(Vec::new()),
        }
    }
}

impl Read for SseStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.buffer.read(out)?;
            if read > 0 {
                return Ok(read);
            }
            match self.rx.recv() {
                Ok(message) => {
                    self.buffer = Cursor::new(format!("data: {message}\n\n").into_bytes());
                }
                Err(_) => return Ok(0),
            }
        }
    }
}
```

Verification:

```sh
cargo test -p peitho server::tests::sync_hub_broadcasts_json_to_subscribers_and_drops_closed_clients
cargo test -p peitho server::tests::sse_stream_formats_data_events
```

### Task 8 - Add `GET /sync` and `POST /sync`

Goal: `/sync` keeps SSE subscribers open on worker threads, and `POST /sync` validates `{index:number}` before broadcasting.

Files:

- `crates/peitho/src/server.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
#[test]
fn present_server_relays_sync_post_to_sse_subscriber() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || {
        server.handle_one();
        server.handle_one();
    });

    let mut sse = TcpStream::connect(addr).unwrap();
    sse.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    sse.write_all(b"GET /sync HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut header_buf = [0_u8; 256];
    let header_len = sse.read(&mut header_buf).unwrap();
    let header = String::from_utf8_lossy(&header_buf[..header_len]);
    assert!(header.contains("200 OK"));
    assert!(header.contains("text/event-stream"));

    let mut post = TcpStream::connect(addr).unwrap();
    post.write_all(
        b"POST /sync HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\n\r\n{\"index\":1}"
    ).unwrap();
    let mut post_response = String::new();
    post.read_to_string(&mut post_response).unwrap();
    assert!(post_response.contains("204 No Content"));

    let mut event_buf = [0_u8; 128];
    let event_len = sse.read(&mut event_buf).unwrap();
    let event = String::from_utf8_lossy(&event_buf[..event_len]);
    assert!(event.contains("data: {\"index\":1}"));

    handle.join().unwrap();
}

#[test]
fn present_server_rejects_invalid_sync_post_body() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("present.html"), "<!doctype html>").unwrap();

    let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
    let addr = server.addr();
    let handle = thread::spawn(move || server.handle_one());

    let mut post = TcpStream::connect(addr).unwrap();
    post.write_all(
        b"POST /sync HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\n\r\n{\"key\":\"x\"}"
    ).unwrap();
    let mut response = String::new();
    post.read_to_string(&mut response).unwrap();

    assert!(response.contains("400 Bad Request"));
    handle.join().unwrap();
}
```

Expected Red:

```text
assertion failed: header.contains("text/event-stream")
```

Implementation:

```rust
// crates/peitho/src/server.rs
use serde::{Deserialize, Serialize};
use std::thread;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncMessage {
    index: usize,
}

impl PresentServer {
    // struct fields:
    // root: PathBuf,
    // server: Server,
    // sync: SyncHub,
    pub fn bind(root: PathBuf, port: u16) -> miette::Result<Self> {
        let server = Server::http(("127.0.0.1", port))
            .map_err(|err| miette::miette!("failed to bind present server: {err}"))?;
        Ok(Self {
            root,
            server,
            sync: SyncHub::default(),
        })
    }

    pub fn serve_forever(self) -> miette::Result<()> {
        for request in self.server.incoming_requests() {
            self.respond(request);
        }
        Ok(())
    }

    pub fn handle_one(&self) {
        if let Some(request) = self.server.incoming_requests().next() {
            self.respond(request);
        }
    }

    fn respond(&self, mut request: tiny_http::Request) {
        match (request.method(), request.url()) {
            (&Method::Get, "/sync") => {
                self.respond_sync_stream(request);
                return;
            }
            (&Method::Post, "/sync") => {
                self.respond_sync_post(request);
                return;
            }
            _ => {}
        }

        if request.method() != &Method::Get {
            send_response(request, Response::empty(StatusCode(405)));
            return;
        }
        self.respond_static(request);
    }

    fn respond_sync_stream(&self, request: tiny_http::Request) {
        let rx = self.sync.subscribe();
        thread::spawn(move || {
            let headers = vec![
                Header::from_bytes("Content-Type", "text/event-stream; charset=utf-8")
                    .expect("valid Content-Type"),
                Header::from_bytes("Cache-Control", "no-cache").expect("valid Cache-Control"),
            ];
            let response = Response::new(StatusCode(200), headers, SseStream::new(rx), None, None);
            send_response(request, response);
        });
    }

    fn respond_sync_post(&self, mut request: tiny_http::Request) {
        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            send_response(
                request,
                Response::from_string("invalid sync body\n").with_status_code(StatusCode(400)),
            );
            return;
        }
        let Ok(message) = serde_json::from_str::<SyncMessage>(&body) else {
            send_response(
                request,
                Response::from_string("invalid sync body\n").with_status_code(StatusCode(400)),
            );
            return;
        };
        let json = serde_json::to_string(&message).expect("SyncMessage serializes");
        self.sync.broadcast(&json);
        send_response(request, Response::empty(StatusCode(204)));
    }

    fn respond_static(&self, request: tiny_http::Request) {
        let Some(path) = resolve_request_path(&self.root, request.url()) else {
            send_response(
                request,
                Response::from_string("404\n").with_status_code(StatusCode(404)),
            );
            return;
        };
        match fs::read(&path) {
            Ok(bytes) => {
                let Ok(header) = Header::from_bytes("Content-Type", content_type(&path)) else {
                    eprintln!("warning: failed to build Content-Type header");
                    return;
                };
                send_response(request, Response::from_data(bytes).with_header(header));
            }
            Err(_) => send_response(
                request,
                Response::from_string("404\n").with_status_code(StatusCode(404)),
            ),
        }
    }
}

fn send_response<R>(request: tiny_http::Request, response: Response<R>)
where
    R: Read + Send + 'static,
{
    if let Err(err) = request.respond(response) {
        eprintln!("warning: failed to send present server response: {err}");
    }
}
```

Update existing server tests that currently consume `PresentServer` through `handle_one(self)`:

```rust
// crates/peitho/tests/present.rs
let server = peitho::server::PresentServer::bind(cache, 0).unwrap();
let addr = server.addr();
let handle = thread::spawn(move || server.handle_one());
```

The static response path, 404 path, 405 path, `/sync` SSE path, and `/sync` POST path all call `send_response`. No request response path calls `request.respond(response)` directly after this task, so a single send failure logs a warning and cannot terminate the presentation server.

Verification:

```sh
cargo test -p peitho --test present present_server_relays_sync_post_to_sse_subscriber
cargo test -p peitho --test present present_server_rejects_invalid_sync_post_body
```

### Task 9 - Add TS Server Sync Channel

Goal: implement a `SyncChannelFactory` backed by `EventSource('/sync')` and `POST /sync`, with injected factories for deterministic tests.

Files:

- `packages/peitho-present/src/sync.ts`
- `packages/peitho-present/test/sync.test.ts`

Test:

```ts
// packages/peitho-present/test/sync.test.ts
import { serverSyncChannelFactory } from "../src/sync";

type MockEventSource = {
  onmessage: ((event: { data: string }) => void) | null;
  close: ReturnType<typeof vi.fn>;
};

it("server sync channel posts local messages to /sync", async () => {
  const source: MockEventSource = { onmessage: null, close: vi.fn() };
  const fetcher = vi.fn(async () => ({ ok: true, status: 204 }) as Response);
  const factory = serverSyncChannelFactory({
    eventSourceFactory: (url) => {
      expect(url).toBe("/sync");
      return source;
    },
    fetcher
  });
  const channel = factory("peitho-sync");

  channel.postMessage({ index: 2 });
  await Promise.resolve();

  expect(fetcher).toHaveBeenCalledWith("/sync", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ index: 2 })
  });
});

it("server sync channel parses eventsource messages", () => {
  const source: MockEventSource = { onmessage: null, close: vi.fn() };
  const factory = serverSyncChannelFactory({
    eventSourceFactory: () => source,
    fetcher: vi.fn()
  });
  const channel = factory("peitho-sync");
  const received: unknown[] = [];
  channel.onmessage = (event) => received.push(event.data);

  source.onmessage?.({ data: "{\"index\":1}" });

  expect(received).toEqual([{ index: 1 }]);
});

it("server sync channel closes eventsource", () => {
  const source: MockEventSource = { onmessage: null, close: vi.fn() };
  const channel = serverSyncChannelFactory({
    eventSourceFactory: () => source,
    fetcher: vi.fn()
  })("peitho-sync");

  channel.close();

  expect(source.close).toHaveBeenCalledTimes(1);
});
```

Expected Red:

```text
Module '"../src/sync"' has no exported member 'serverSyncChannelFactory'
```

Implementation:

```ts
// packages/peitho-present/src/sync.ts
export type EventSourceLike = {
  onmessage: ((event: { data: string }) => void) | null;
  close(): void;
};

export type ServerSyncOptions = {
  url?: string;
  eventSourceFactory?: (url: string) => EventSourceLike;
  fetcher?: typeof fetch;
};

export function serverSyncChannelFactory(options: ServerSyncOptions = {}): SyncChannelFactory {
  const url = options.url ?? "/sync";
  const eventSourceFactory = options.eventSourceFactory ?? ((nextUrl) => new EventSource(nextUrl));
  const fetcher = options.fetcher ?? fetch.bind(globalThis);

  return () => {
    const source = eventSourceFactory(url);
    let onmessage: ((event: { data: unknown }) => void) | null = null;
    source.onmessage = (event): void => {
      try {
        onmessage?.({ data: JSON.parse(event.data) });
      } catch {
        console.error("Invalid peitho server sync message");
      }
    };
    return {
      get onmessage() {
        return onmessage;
      },
      set onmessage(next) {
        onmessage = next;
      },
      postMessage(message: SyncMessage): void {
        void fetcher(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(message)
        }).then((response) => {
          if (!response.ok) console.error(`Failed to post sync message: ${response.status}`);
        }).catch((error) => {
          console.error(`Failed to post sync message: ${String(error)}`);
        });
      },
      close(): void {
        source.close();
      }
    };
  };
}
```

Verification:

```sh
cd packages/peitho-present
npm test -- sync.test.ts
npm run typecheck
```

### Task 10 - Wire Generated HTML to Server Sync

Goal: `present.html` and `presenter.html` use the server sync factory; generated HTML no longer uses the default BroadcastChannel factory.

Files:

- `crates/peitho-core/src/render.rs`
- `crates/peitho/tests/present.rs`
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/generated.test.ts`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn present_index_uses_server_sync_factory() {
    let html = render_present_index();

    assert!(html.contains("serverSyncChannelFactory"));
    assert!(html.contains("installSyncBridge(window, serverSyncChannelFactory())"));
    assert!(!html.contains("installSyncBridge(window);"));
}

#[test]
fn presenter_index_passes_server_sync_factory_to_presenter_view() {
    let html = render_presenter_index();

    assert!(html.contains("serverSyncChannelFactory"));
    assert!(html.contains("syncChannelFactory: serverSyncChannelFactory()"));
}
```

Expected Red:

```text
assertion failed: html.contains("serverSyncChannelFactory")
```

Implementation:

```rust
// crates/peitho-core/src/render.rs, inside render_present_index import block
import {
  installCanvasClickNavigation,
  installFullscreenShortcut,
  installKeyboardNavigation,
  installPresentationControls,
  installSyncBridge,
  mountPresentShell,
  serverSyncChannelFactory
} from './shell.js';

// inside present main()
installSyncBridge(window, serverSyncChannelFactory());

// crates/peitho-core/src/render.rs, presenter import block
import { mountPresenterView, serverSyncChannelFactory } from './shell.js';

// inside presenter main()
await mountPresenterView({
  root,
  notes,
  syncChannelFactory: serverSyncChannelFactory()
});
```

```ts
// packages/peitho-present/src/index.ts
export { installSyncBridge, serverSyncChannelFactory } from "./sync";
export type {
  EventSourceLike,
  ServerSyncOptions,
  SyncChannel,
  SyncChannelFactory,
  SyncMessage
} from "./sync";
```

Verification:

```sh
cargo test -p peitho-core render::tests::present_index_uses_server_sync_factory
cargo test -p peitho-core render::tests::presenter_index_passes_server_sync_factory_to_presenter_view
cd packages/peitho-present && npm test -- generated.test.ts
```

### Task 11 - Remove Window Management API Placement Path

Goal: remove `getScreenDetails`, `requestFullscreen({screen})`, `placeWindows`, overlay retry, and `window-management.d.ts`; keep only popup opening for manual presenter launch on one-window fallback.

Files:

- `packages/peitho-present/src/presentDisplay.ts`
- `packages/peitho-present/src/window-management.d.ts`
- `packages/peitho-present/src/controls.ts`
- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/presentDisplay.test.ts`
- `packages/peitho-present/test/controls.test.ts`
- `packages/peitho-present/test/generated.test.ts`

Test:

```ts
// packages/peitho-present/test/presentDisplay.test.ts
import { fallbackFeatures, openPresenterPopup } from "../src/presentDisplay";

it("opens presenter popup only", () => {
  const popup = { close: vi.fn() } as Window;
  const openWindow = vi.fn(() => popup);

  expect(openPresenterPopup({ openWindow })).toBe(popup);
  expect(openWindow).toHaveBeenCalledWith(
    "presenter.html",
    "peitho-presenter",
    "popup=yes,width=1200,height=800,left=80,top=80"
  );
});

it("keeps fallback popup features stable", () => {
  expect(fallbackFeatures()).toBe("popup=yes,width=1200,height=800,left=80,top=80");
});
```

Expected Red:

```text
Module '"../src/presentDisplay"' has no exported member 'openPresenterPopup'
```

Implementation:

```ts
// packages/peitho-present/src/presentDisplay.ts
export const PRESENTER_URL = "presenter.html";
export const PRESENTER_TARGET = "peitho-presenter";

export type OpenPresenterPopupOptions = {
  window?: Window;
  url?: string;
  openWindow?: (url: string, target: string, features: string) => Window | null;
};

export function fallbackFeatures(): string {
  return "popup=yes,width=1200,height=800,left=80,top=80";
}

export function openPresenterPopup(options: OpenPresenterPopupOptions = {}): Window | null {
  const win = options.window ?? window;
  const url = options.url ?? PRESENTER_URL;
  const openWindow =
    options.openWindow ?? ((nextUrl, target, features) => win.open(nextUrl, target, features));
  return openWindow(url, PRESENTER_TARGET, fallbackFeatures());
}
```

```ts
// packages/peitho-present/src/controls.ts
import { openPresenterPopup, type OpenPresenterPopupOptions } from "./presentDisplay";

export type PresentationControlsOptions = {
  root: HTMLElement;
  window?: Window;
  document?: Document;
  bus?: EventTarget;
  idleMs?: number;
  openPresenter?: () => void | Promise<void>;
  openPresenterWindow?: OpenPresenterPopupOptions["openWindow"];
  closeWindow?: () => void;
};

const openPresenter =
  options.openPresenter ??
  (() =>
    openPresenterPopup({
      window: win,
      openWindow: options.openPresenterWindow
    }));
```

Delete `packages/peitho-present/src/window-management.d.ts`. Remove `src/**/*.d.ts` from `tsconfig.json` only if no other `.d.ts` files remain.

Verification:

```sh
cd packages/peitho-present
npm test -- presentDisplay.test.ts controls.test.ts generated.test.ts
npm run typecheck
rg -n "getScreenDetails|ScreenDetailed|placeWindows|peitho-place-overlay|requestFullscreen\\?\\.\\(" src test
```

### Task 12 - Add Close Controls

Goal: slide controls and presenter view expose Close buttons that only call injectable `closeWindow`.

Files:

- `packages/peitho-present/src/controls.ts`
- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/test/controls.test.ts`
- `packages/peitho-present/test/presenter.test.ts`

Test:

```ts
// packages/peitho-present/test/controls.test.ts
it("close button calls the injected close window function", () => {
  const root = document.createElement("main");
  const closeWindow = vi.fn();
  const cleanup = installPresentationControls({
    root,
    window,
    document,
    bus: window,
    closeWindow
  });
  cleanups.push(cleanup);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="close"]')?.click();

  expect(closeWindow).toHaveBeenCalledTimes(1);
});

// packages/peitho-present/test/presenter.test.ts
it("presenter close button calls the injected close window function", async () => {
  const root = document.createElement("main");
  const closeWindow = vi.fn();
  const view = await mountPresenterView({
    root,
    notes: { version: 1, notes: {} },
    fetcher: standardFetch(),
    window,
    document,
    syncChannelFactory: () => mockChannel(),
    closeWindow
  });
  views.push(view);

  root.querySelector<HTMLButtonElement>('[data-peitho-action="close"]')?.click();

  expect(closeWindow).toHaveBeenCalledTimes(1);
});
```

Expected Red:

```text
expected "vi.fn()" to be called 1 times, but got 0 times
```

Implementation:

```ts
// packages/peitho-present/src/controls.ts
const closeWindow = options.closeWindow ?? (() => win.close());
bar.innerHTML = [
  '<button type="button" data-peitho-action="prev" aria-label="Previous slide">◀</button>',
  '<button type="button" data-peitho-action="next" aria-label="Next slide">▶</button>',
  '<output data-peitho-control="counter">– / –</output>',
  '<button type="button" data-peitho-action="fullscreen" aria-label="Toggle fullscreen">⛶</button>',
  '<button type="button" data-peitho-action="presenter">Presenter</button>',
  '<button type="button" data-peitho-action="close" aria-label="Close presentation">✕</button>'
].join("");

if (action === "close") closeWindow();
```

```ts
// packages/peitho-present/src/presenter.ts
export type PresenterOptions = {
  root: HTMLElement;
  notes: Notes;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  now?: () => number;
  syncChannelFactory?: SyncChannelFactory;
  closeWindow?: () => void;
};

// controls markup
<button type="button" data-peitho-action="close">Close</button>

// after timer control listeners
const closeWindow = options.closeWindow ?? (() => win.close());
options.root.querySelector('[data-peitho-action="close"]')?.addEventListener("click", () => {
  closeWindow();
});
```

Verification:

```sh
cd packages/peitho-present
npm test -- controls.test.ts presenter.test.ts
npm run typecheck
```

### Task 13 - Update Present Cache and Repository Smoke Expectations

Goal: smoke tests reflect server sync, split-profile browser planning, and removal of Window Management API markers.

Files:

- `crates/peitho/tests/present.rs`
- `crates/peitho/src/browser.rs`
- `crates/peitho-core/src/render.rs`

Test:

```rust
// crates/peitho/tests/present.rs, repository_example_present_no_serve_smoke assertions
assert!(present_html.contains("serverSyncChannelFactory"));
assert!(present_html.contains("installSyncBridge(window, serverSyncChannelFactory())"));
assert!(present_html.contains(r#"data-peitho-action="close""#));
assert!(presenter_html.contains("syncChannelFactory: serverSyncChannelFactory()"));
assert!(shell_js.contains("serverSyncChannelFactory"));
assert!(!shell_js.contains("getScreenDetails"));
assert!(!shell_js.contains("data-peitho-place-overlay"));
assert!(!shell_js.contains("requestFullscreen({ screen"));
```

Expected Red:

```text
assertion failed: shell_js.contains("serverSyncChannelFactory")
```

Implementation:

- Replace M9 assertions for `openPresenterWithDisplay`, `getScreenDetails`, and `requestFullscreen` with server sync and Close markers.
- Update shell fixtures in `present_no_serve_writes_clean_present_cache` and related tests to export `serverSyncChannelFactory` when the rendered HTML imports it.

Verification:

```sh
cargo test -p peitho --test present repository_example_present_no_serve_smoke
cargo test -p peitho --test present present_no_serve_writes_clean_present_cache
```

### Task 14 - Full Gate and Manual Verification Markers

Goal: run all project gates and report the launch/sync markers needed for real-device verification.

Files:

- `crates/peitho/src/displays.rs`
- `crates/peitho/src/browser.rs`
- `crates/peitho/src/server.rs`
- `packages/peitho-present/dist/shell.js`

Test:

```rust
// crates/peitho/src/browser.rs
#[test]
fn macos_two_window_command_report_matches_measured_strategy() {
    let env = BrowserEnvironment {
        platform: BrowserPlatform::Macos,
        mac_google_chrome_available: true,
        linux_browser: None,
        chrome_profiles: Some(test_profiles()),
        layout: Some(test_layout()),
    };

    let rendered = plan_browser_commands(&test_request(false), &env)
        .into_iter()
        .map(|command| {
            std::iter::once(command.program.to_string_lossy().to_string())
                .chain(command.args.iter().map(|arg| arg.to_string_lossy().to_string()))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>();

    println!("{}", rendered.join("\n"));
    assert!(rendered[0].contains("--user-data-dir=/Users/alice/.peitho/chrome-profile-slides"));
    assert!(rendered[0].contains("--window-position=-1055,0"));
    assert!(rendered[0].contains("--start-fullscreen"));
    assert!(rendered[1].contains("--user-data-dir=/Users/alice/.peitho/chrome-profile-presenter"));
    assert!(rendered[1].contains("--window-position=156,91"));
    assert!(rendered[1].contains("--window-size=1200,800"));
    assert!(!rendered[1].contains("--start-fullscreen"));
}
```

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
cargo test -p peitho browser::tests::macos_two_window_command_report_matches_measured_strategy -- --nocapture
rg -n "serverSyncChannelFactory|EventSource|data-peitho-action=\\\"close\\\"" packages/peitho-present/dist/shell.js .peitho/present-cache/present.html .peitho/present-cache/presenter.html
rg -n "getScreenDetails|data-peitho-place-overlay|requestFullscreen\\(\\{ screen" packages/peitho-present/src packages/peitho-present/test packages/peitho-present/dist/shell.js
```

Report these concrete markers:

```text
Slides command includes:
--user-data-dir=$HOME/.peitho/chrome-profile-slides --window-position=<external-x>,<external-y> --start-fullscreen --app=<present.html>

Presenter command includes:
--user-data-dir=$HOME/.peitho/chrome-profile-presenter --window-position=<primary-x>,<primary-y> --window-size=<clamped> --app=<presenter.html>

Generated HTML:
present.html and presenter.html use serverSyncChannelFactory; shell.js has no Window Management API placement path.
```

## Summary

This plan has 14 TDD tasks. It starts by making macOS display detection and coordinate conversion pure and testable, then moves browser launch planning to split Chrome profiles with deterministic two-window commands. It wires the CLI and local server to use that plan, adds `/sync` SSE plus `POST /sync` as the cross-profile transport, switches generated HTML and TS sync to the server transport, removes the Window Management API placement path, adds Close controls, and finishes with full gates plus command/output markers for real-device verification.
