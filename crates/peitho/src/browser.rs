use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crate::cdp;
use crate::displays::{self, PresentationLayout, SavedWindowBounds, WindowPlacement};
use crate::labels::LabelStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserPlatform {
    Macos,
    Linux,
    Other,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserCommand {
    pub role: WindowRole,
    pub program: OsString,
    pub args: Vec<OsString>,
    pub fullscreen: Option<CdpFullscreen>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRole {
    Slides,
    Presenter,
}

impl WindowRole {
    fn label(self) -> &'static str {
        match self {
            WindowRole::Slides => "slides",
            WindowRole::Presenter => "presenter",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserPlan {
    pub commands: Vec<BrowserCommand>,
    pub opens_presenter: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct BrowserOpenRequest<'a> {
    pub slides_url: &'a str,
    pub presenter_url: &'a str,
    pub no_presenter: bool,
}

/// Chrome keys per-app window placement by an app name derived from the URL
/// (host + path). Dots in that name ("127.0.0.1", ".html") get expanded as
/// nested pref paths on write and never match on read, so placement is
/// silently never restored. A dot-free URL — localhost host, extensionless
/// /presenter route — keeps the key flat and lets Chrome restore the
/// presenter window where it was last closed.
pub fn presenter_url(slides_url: &str) -> String {
    slides_url
        .replace("127.0.0.1", "localhost")
        .replace("/present.html", "/presenter")
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

/// A window peitho fullscreens over CDP once Chrome is up. Chrome 155 ignores
/// `--start-fullscreen` and off-primary `--window-position` (Issue #683), so no
/// launch flag asks for fullscreen; only `chrome_launch` builds this, together
/// with the `--remote-debugging-port=0` flag it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdpFullscreen {
    profile: PathBuf,
    position: Option<(i32, i32)>,
}

struct ChromeLaunch {
    args: Vec<OsString>,
    fullscreen: Option<CdpFullscreen>,
}

/// `None` placement is the single-window case: fullscreen wherever Chrome
/// opens the window.
fn chrome_launch(
    profile_dir: &Path,
    url: &str,
    placement: Option<WindowPlacement>,
) -> ChromeLaunch {
    let mut args = chrome_base_args(profile_dir, url);
    let fullscreen_at = match placement {
        None => Some(None),
        Some(WindowPlacement::Fullscreen { x, y }) => Some(Some((x, y))),
        Some(WindowPlacement::Windowed {
            x,
            y,
            width,
            height,
        }) => {
            args.push(OsString::from(format!("--window-position={x},{y}")));
            args.push(OsString::from(format!("--window-size={width},{height}")));
            None
        }
        Some(WindowPlacement::Restored) => None,
    };
    let fullscreen = fullscreen_at.map(|position| {
        args.push(OsString::from("--remote-debugging-port=0"));
        CdpFullscreen {
            profile: profile_dir.to_path_buf(),
            position,
        }
    });
    ChromeLaunch { args, fullscreen }
}

fn macos_chrome_command(role: WindowRole, launch: ChromeLaunch) -> BrowserCommand {
    let mut full_args = vec![
        OsString::from("-na"),
        OsString::from("Google Chrome"),
        OsString::from("--args"),
    ];
    full_args.extend(launch.args);
    BrowserCommand {
        role,
        program: OsString::from("open"),
        args: full_args,
        fullscreen: launch.fullscreen,
    }
}

fn linux_chrome_command(role: WindowRole, program: &OsStr, launch: ChromeLaunch) -> BrowserCommand {
    BrowserCommand {
        role,
        program: program.to_owned(),
        args: launch.args,
        fullscreen: launch.fullscreen,
    }
}

fn open_command(program: &str, url: &str) -> BrowserCommand {
    BrowserCommand {
        role: WindowRole::Slides,
        program: OsString::from(program),
        args: vec![OsString::from(url)],
        fullscreen: None,
    }
}

pub fn plan_browser_commands(
    request: &BrowserOpenRequest<'_>,
    env: &BrowserEnvironment,
) -> Vec<BrowserCommand> {
    match env.platform {
        BrowserPlatform::Macos if env.mac_google_chrome_available => {
            let Some(profiles) = env.chrome_profiles.as_ref() else {
                return vec![open_command("open", request.slides_url)];
            };
            if let Some(layout) = env.layout.filter(|_| !request.no_presenter) {
                return vec![
                    macos_chrome_command(
                        WindowRole::Slides,
                        chrome_launch(&profiles.slides, request.slides_url, Some(layout.slides)),
                    ),
                    macos_chrome_command(
                        WindowRole::Presenter,
                        chrome_launch(
                            &profiles.presenter,
                            request.presenter_url,
                            Some(layout.presenter),
                        ),
                    ),
                ];
            }
            vec![macos_chrome_command(
                WindowRole::Slides,
                chrome_launch(&profiles.slides, request.slides_url, None),
            )]
        }
        BrowserPlatform::Macos => vec![open_command("open", request.slides_url)],
        BrowserPlatform::Linux => linux_browser_commands(request, env),
        BrowserPlatform::Other => Vec::new(),
    }
}

fn linux_browser_commands(
    request: &BrowserOpenRequest<'_>,
    env: &BrowserEnvironment,
) -> Vec<BrowserCommand> {
    let (Some(program), Some(profiles)) =
        (env.linux_browser.as_deref(), env.chrome_profiles.as_ref())
    else {
        return vec![open_command("xdg-open", request.slides_url)];
    };

    if let Some(layout) = env.layout.filter(|_| !request.no_presenter) {
        return vec![
            linux_chrome_command(
                WindowRole::Slides,
                program,
                chrome_launch(&profiles.slides, request.slides_url, Some(layout.slides)),
            ),
            linux_chrome_command(
                WindowRole::Presenter,
                program,
                chrome_launch(
                    &profiles.presenter,
                    request.presenter_url,
                    Some(layout.presenter),
                ),
            ),
        ];
    }

    vec![linux_chrome_command(
        WindowRole::Slides,
        program,
        chrome_launch(&profiles.slides, request.slides_url, None),
    )]
}

pub fn plan_browser(request: &BrowserOpenRequest<'_>, env: &BrowserEnvironment) -> BrowserPlan {
    BrowserPlan::from_commands(plan_browser_commands(request, env))
}

impl BrowserPlan {
    fn from_commands(commands: Vec<BrowserCommand>) -> Self {
        let opens_presenter = commands
            .iter()
            .any(|command| command.role == WindowRole::Presenter);
        Self {
            commands,
            opens_presenter,
        }
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
        chrome_profiles: chrome_profiles_from_home(std::env::var_os("HOME")),
        layout: None,
    }
}

/// The Chrome app name for the presenter URL; placement is stored under this
/// key in the profile's Preferences. Must stay dot-free (see presenter_url).
const PRESENTER_APP_PLACEMENT_KEY: &str = "localhost_/presenter";

/// Read the presenter window bounds Chrome saved in the peitho presenter
/// profile, if any. Used to decide between letting Chrome restore the window
/// (bounds exist and are visible) and seeding an explicit first-run position.
pub fn saved_presenter_bounds(profiles: &ChromeProfiles) -> Option<SavedWindowBounds> {
    let path = profiles.presenter.join("Default/Preferences");
    let json = std::fs::read_to_string(path).ok()?;
    let prefs: serde_json::Value = serde_json::from_str(&json).ok()?;
    let placement = prefs
        .get("browser")?
        .get("app_window_placement")?
        .get(PRESENTER_APP_PLACEMENT_KEY)?;
    let left = placement.get("left")?.as_i64()? as i32;
    let top = placement.get("top")?.as_i64()? as i32;
    let right = placement.get("right")?.as_i64()? as i32;
    let bottom = placement.get("bottom")?.as_i64()? as i32;
    Some(SavedWindowBounds {
        x: left,
        y: top,
        width: u32::try_from(right.checked_sub(left)?).ok()?,
        height: u32::try_from(bottom.checked_sub(top)?).ok()?,
    })
}

fn stale_profile_patterns(profiles: &ChromeProfiles) -> [String; 2] {
    [
        format!("--user-data-dir={}", profiles.slides.display()),
        format!("--user-data-dir={}", profiles.presenter.display()),
    ]
}

/// Extract the pids of browser main processes whose command line holds one
/// of the peitho profile dirs. Child processes (`--type=renderer` etc.) are
/// excluded: quitting the main process takes them down with it. Matching on
/// the full `ps` command line instead of `pgrep -f` keeps shells that merely
/// mention the profile path out of the result; a non-GUI false positive is
/// additionally ignored by `NSRunningApplication` returning nil.
fn stale_main_pids(ps_output: &str, patterns: &[String]) -> Vec<String> {
    ps_output
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim_start().split_once(' ')?;
            let is_main = patterns.iter().any(|pattern| command.contains(pattern))
                && !command.contains("--type=");
            is_main.then(|| pid.to_owned())
        })
        .collect()
}

fn profile_main_pids(patterns: &[String]) -> Vec<String> {
    Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| stale_main_pids(&String::from_utf8_lossy(&output.stdout), patterns))
        .unwrap_or_default()
}

/// JXA quirk: the ObjC bridge invokes the zero-arg `terminate` method on
/// property access and returns its BOOL, so the call is written without
/// parentheses. If a macOS version bridges it as a plain function instead,
/// the access is a no-op and the pkill escalation below still cleans up.
fn graceful_quit_jxa(pid: &str) -> String {
    format!(
        "ObjC.import('AppKit'); \
         const app = $.NSRunningApplication.runningApplicationWithProcessIdentifier({pid}); \
         if (!app.isNil()) app.terminate;"
    )
}

/// Ask the instance to quit the way the user would (Quit Apple Event on
/// macOS, SIGTERM elsewhere). A raw kill is recorded by Chrome as a crash
/// (`exit_type: Crashed`) and the next launch runs crash restore, which
/// resurrects stale session windows and bounds over the saved placement.
fn request_graceful_quit(pid: &str) {
    // output() rather than status(): osascript echoes the value of the last
    // JXA expression (`true` from terminate) to stdout, which would leak
    // into the present command's own output.
    if cfg!(target_os = "macos") {
        let _ = Command::new("osascript")
            .args(["-l", "JavaScript", "-e", &graceful_quit_jxa(pid)])
            .output();
    } else {
        let _ = Command::new("kill").args(["--", pid]).output();
    }
}

/// Quit any Chrome instances still holding the peitho profiles. Called when
/// the presentation ends so no windowless Chrome lingers in the Dock between
/// sessions; the launch path stays as a fallback for sessions that never
/// ended cleanly.
pub fn quit_profile_instances() {
    if let Some(profiles) = chrome_profiles_from_home(std::env::var_os("HOME")) {
        terminate_stale_profile_instances(&profiles);
    }
}

/// Chrome on macOS keeps running after its last window closes, so a previous
/// `present` session leaves processes holding the peitho profiles. Launching
/// into such a process hands off the URL and drops every flag except `--app`
/// (window position, size, and fullscreen are silently ignored). Quit the
/// stale processes so each session starts fresh ones that honor the flags
/// and restore saved window placement from a cleanly exited profile.
fn terminate_stale_profile_instances(profiles: &ChromeProfiles) {
    let patterns = stale_profile_patterns(profiles);
    let pids = profile_main_pids(&patterns);
    if pids.is_empty() {
        return;
    }
    for pid in &pids {
        request_graceful_quit(pid);
    }
    // A clean Chrome shutdown can take a few seconds; escalating too early
    // turns it back into the crash-exit this function exists to avoid.
    for attempt in 0..100 {
        if profile_main_pids(&patterns).is_empty() {
            return;
        }
        if attempt == 60 {
            for pattern in &patterns {
                let _ = Command::new("pkill").args(["-f", "--", pattern]).status();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn prepare_profile_dirs(profiles: Option<&ChromeProfiles>) -> bool {
    let Some(profiles) = profiles else {
        return false;
    };
    for profile in [&profiles.slides, &profiles.presenter] {
        if let Err(err) = std::fs::create_dir_all(profile) {
            eprintln!(
                "{}failed to prepare Chrome profile at {}: {err}",
                LabelStyle::for_stderr().warning(),
                profile.display()
            );
            return false;
        }
    }
    true
}

fn presenter_mode(
    presenter_windowed: bool,
    profiles: Option<&ChromeProfiles>,
) -> displays::PresenterMode {
    if presenter_windowed {
        displays::PresenterMode::Windowed {
            saved: profiles.and_then(saved_presenter_bounds),
        }
    } else {
        displays::PresenterMode::Fullscreen
    }
}

pub fn plan_browser_with_request(
    request: BrowserOpenRequest<'_>,
    presenter_windowed: bool,
) -> BrowserPlan {
    let mut env = current_environment();
    env.layout = displays::detect_presentation_layout(presenter_mode(
        presenter_windowed,
        env.chrome_profiles.as_ref(),
    ));
    if !prepare_profile_dirs(env.chrome_profiles.as_ref()) {
        env.chrome_profiles = None;
    }
    if let Some(profiles) = env.chrome_profiles.as_ref() {
        terminate_stale_profile_instances(profiles);
    }

    plan_browser(&request, &env)
}

pub fn open_browser_plan(plan: BrowserPlan) {
    if plan.commands.is_empty() {
        eprintln!(
            "{}browser auto-open is not supported on this platform",
            LabelStyle::for_stderr().warning()
        );
        return;
    }
    let mut fullscreens = Vec::new();
    for command in plan.commands {
        if let Some(fullscreen) = &command.fullscreen {
            // A previous session's file would point at a dead port. Ignoring a
            // failed removal is safe: a stale port fails loudly below.
            let _ = std::fs::remove_file(fullscreen.profile.join(cdp::DEVTOOLS_PORT_FILE));
        }
        if let Err(err) = Command::new(&command.program).args(&command.args).spawn() {
            eprintln!(
                "{}failed to open browser with {}: {err}",
                LabelStyle::for_stderr().warning(),
                command.program.to_string_lossy()
            );
            continue;
        }
        if let Some(fullscreen) = command.fullscreen {
            fullscreens.push((command.role, fullscreen));
        }
    }
    if fullscreens.is_empty() {
        return;
    }
    // One window at a time: a fullscreen transition started while another
    // is running falls back to a normal window (measured, Issue #683). Off
    // the main thread so the server starts serving meanwhile.
    std::thread::spawn(move || {
        for (role, fullscreen) in fullscreens {
            if let Err(err) = apply_fullscreen(&fullscreen) {
                eprintln!(
                    "{}failed to fullscreen the {} window: {err}",
                    LabelStyle::for_stderr().warning(),
                    role.label()
                );
            }
        }
    });
}

const FULLSCREEN_TIMEOUT: Duration = Duration::from_secs(20);

fn apply_fullscreen(target: &CdpFullscreen) -> miette::Result<()> {
    let deadline = Instant::now() + FULLSCREEN_TIMEOUT;
    let port = cdp::wait_for_devtools_port(&target.profile, deadline, || Ok(()))?;
    let url = cdp::fetch_page_websocket_url(port, deadline)?;
    let mut client = cdp::CdpClient::connect(port, &url, deadline)?;
    client.fullscreen_window(target.position, deadline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::displays::{PresentationLayout, WindowPlacement};
    use std::{ffi::OsString, path::PathBuf};

    fn test_profiles() -> ChromeProfiles {
        ChromeProfiles {
            slides: PathBuf::from("/Users/alice/.peitho/chrome-profile-slides"),
            presenter: PathBuf::from("/Users/alice/.peitho/chrome-profile-presenter"),
        }
    }

    fn test_layout() -> PresentationLayout {
        PresentationLayout {
            slides: WindowPlacement::Fullscreen { x: -1055, y: 0 },
            presenter: WindowPlacement::Fullscreen { x: 156, y: 91 },
        }
    }

    fn windowed_presenter_layout() -> PresentationLayout {
        let mut layout = test_layout();
        layout.presenter = WindowPlacement::Restored;
        layout
    }

    fn test_request(no_presenter: bool) -> BrowserOpenRequest<'static> {
        BrowserOpenRequest {
            slides_url: "http://127.0.0.1:8000/present.html",
            presenter_url: "http://127.0.0.1:8000/presenter.html",
            no_presenter,
        }
    }

    #[test]
    fn chrome_profiles_are_split_by_window_role() {
        assert_eq!(
            chrome_profiles_from_home(Some(OsString::from("/Users/alice"))),
            Some(test_profiles())
        );
    }

    #[test]
    fn macos_single_window_uses_slides_profile() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: true,
            linux_browser: None,
            chrome_profiles: Some(test_profiles()),
            layout: None,
        };

        let commands = plan_browser_commands(&test_request(false), &env);

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
                OsString::from("--remote-debugging-port=0"),
            ]
        );
        assert_eq!(
            commands[0].fullscreen,
            Some(CdpFullscreen {
                profile: test_profiles().slides,
                position: None,
            })
        );
    }

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
        assert_eq!(commands[0].role, WindowRole::Slides);
        assert_eq!(commands[1].role, WindowRole::Presenter);
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
                OsString::from("--remote-debugging-port=0"),
            ]
        );
        assert_eq!(
            commands[0].fullscreen,
            Some(CdpFullscreen {
                profile: test_profiles().slides,
                position: Some((-1055, 0)),
            })
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
                OsString::from("--remote-debugging-port=0"),
            ]
        );
        assert_eq!(
            commands[1].fullscreen,
            Some(CdpFullscreen {
                profile: test_profiles().presenter,
                position: Some((156, 91)),
            })
        );
    }

    #[test]
    fn windowed_presenter_placement_passes_position_and_size() {
        let mut layout = test_layout();
        layout.presenter = WindowPlacement::Windowed {
            x: 156,
            y: 91,
            width: 1200,
            height: 800,
        };
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: true,
            linux_browser: None,
            chrome_profiles: Some(test_profiles()),
            layout: Some(layout),
        };

        let commands = plan_browser_commands(&test_request(false), &env);

        assert!(commands[1]
            .args
            .contains(&OsString::from("--window-position=156,91")));
        assert!(commands[1]
            .args
            .contains(&OsString::from("--window-size=1200,800")));
        assert!(!commands[1]
            .args
            .contains(&OsString::from("--remote-debugging-port=0")));
        assert_eq!(commands[1].fullscreen, None);
    }

    #[test]
    fn saved_presenter_bounds_read_flat_localhost_key() {
        let temp = tempfile::tempdir().expect("temp dir");
        let profiles = ChromeProfiles {
            slides: temp.path().join("slides"),
            presenter: temp.path().join("presenter"),
        };
        std::fs::create_dir_all(profiles.presenter.join("Default")).unwrap();
        std::fs::write(
            profiles.presenter.join("Default/Preferences"),
            r#"{"browser":{"app_window_placement":{"localhost_/presenter":{"left":300,"top":60,"right":1500,"bottom":960}}}}"#,
        )
        .unwrap();

        assert_eq!(
            saved_presenter_bounds(&profiles),
            Some(SavedWindowBounds {
                x: 300,
                y: 60,
                width: 1200,
                height: 900,
            })
        );
    }

    #[test]
    fn saved_presenter_bounds_none_without_preferences_or_key() {
        let temp = tempfile::tempdir().expect("temp dir");
        let profiles = ChromeProfiles {
            slides: temp.path().join("slides"),
            presenter: temp.path().join("presenter"),
        };

        assert_eq!(saved_presenter_bounds(&profiles), None);

        std::fs::create_dir_all(profiles.presenter.join("Default")).unwrap();
        std::fs::write(
            profiles.presenter.join("Default/Preferences"),
            r#"{"browser":{"app_window_placement":{}}}"#,
        )
        .unwrap();

        assert_eq!(saved_presenter_bounds(&profiles), None);
    }

    #[test]
    fn restored_presenter_placement_passes_no_placement_flags() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: true,
            linux_browser: None,
            chrome_profiles: Some(test_profiles()),
            layout: Some(windowed_presenter_layout()),
        };

        let commands = plan_browser_commands(&test_request(false), &env);

        assert_eq!(commands.len(), 2);
        assert!(commands[0].fullscreen.is_some());
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
            ]
        );
        assert_eq!(commands[1].fullscreen, None);
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
        assert_eq!(commands[0].role, WindowRole::Slides);
        assert_eq!(
            commands[0].fullscreen,
            Some(CdpFullscreen {
                profile: test_profiles().slides,
                position: None,
            })
        );
        assert!(!commands[0]
            .args
            .iter()
            .any(|arg| arg == "--window-size=1200,800"));
    }

    #[test]
    fn browser_plan_reports_presenter_open_for_two_display_chrome_plan() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: true,
            linux_browser: None,
            chrome_profiles: Some(test_profiles()),
            layout: Some(test_layout()),
        };

        let plan = plan_browser(&test_request(false), &env);

        assert!(plan.opens_presenter);
        assert_eq!(plan.commands.len(), 2);
        assert_eq!(plan.commands[1].role, WindowRole::Presenter);
    }

    #[test]
    fn browser_plan_reports_no_presenter_when_chrome_is_unavailable() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: false,
            linux_browser: None,
            chrome_profiles: Some(test_profiles()),
            layout: Some(test_layout()),
        };

        let plan = plan_browser(&test_request(false), &env);

        assert!(!plan.opens_presenter);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].role, WindowRole::Slides);
    }

    #[test]
    fn browser_plan_reports_no_presenter_when_disabled_by_flag() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: true,
            linux_browser: None,
            chrome_profiles: Some(test_profiles()),
            layout: Some(test_layout()),
        };

        let plan = plan_browser(&test_request(true), &env);

        assert!(!plan.opens_presenter);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(plan.commands[0].role, WindowRole::Slides);
    }

    #[test]
    fn linux_falls_back_to_xdg_open_without_chrome_or_chromium() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Linux,
            mac_google_chrome_available: false,
            linux_browser: None,
            chrome_profiles: Some(test_profiles()),
            layout: None,
        };

        let commands = plan_browser_commands(&test_request(false), &env);

        assert_eq!(commands[0].program, OsString::from("xdg-open"));
        assert_eq!(
            commands[0].args,
            vec![OsString::from("http://127.0.0.1:8000/present.html")]
        );
    }

    #[test]
    fn no_launch_flag_asks_chrome_for_fullscreen_or_off_primary_position() {
        let request = test_request(false);
        let mut commands = Vec::new();
        for (platform, layout) in [
            (BrowserPlatform::Macos, Some(test_layout())),
            (BrowserPlatform::Macos, None),
            (BrowserPlatform::Linux, Some(test_layout())),
            (BrowserPlatform::Linux, None),
        ] {
            let env = BrowserEnvironment {
                platform,
                mac_google_chrome_available: true,
                linux_browser: Some(OsString::from("google-chrome")),
                chrome_profiles: Some(test_profiles()),
                layout,
            };
            commands.extend(plan_browser_commands(&request, &env));
        }

        for command in commands {
            assert!(command.fullscreen.is_some());
            assert!(command.args.iter().all(|arg| arg != "--start-fullscreen"
                && !arg.to_string_lossy().starts_with("--window-position")));
        }
    }

    #[test]
    fn presenter_url_uses_dotless_localhost_app_name() {
        assert_eq!(
            presenter_url("http://127.0.0.1:49152/present.html"),
            "http://localhost:49152/presenter"
        );
    }

    #[test]
    fn graceful_quit_script_targets_pid_via_nsrunningapplication() {
        let script = graceful_quit_jxa("12345");
        assert!(script.contains("NSRunningApplication"));
        assert!(script.contains("runningApplicationWithProcessIdentifier(12345)"));
        assert!(script.contains("app.terminate;"));
    }

    #[test]
    fn stale_main_pids_keep_browser_main_and_drop_children_and_shells() {
        let patterns = stale_profile_patterns(&test_profiles());
        let ps_output = "\
  101 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome --user-data-dir=/Users/alice/.peitho/chrome-profile-slides --app=http://x/present.html
  102 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome --type=renderer --user-data-dir=/Users/alice/.peitho/chrome-profile-slides
  103 bash -c pgrep -f -- --user-data-dir=/Users/alice/.peitho/other-profile
  104 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome --user-data-dir=/Users/alice/.peitho/chrome-profile-presenter --app=http://x/presenter.html
  105 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome --user-data-dir=/Users/alice/.config/chrome-other
";

        assert_eq!(stale_main_pids(ps_output, &patterns), vec!["101", "104"]);
    }

    #[test]
    fn stale_profile_patterns_target_only_peitho_profile_dirs() {
        assert_eq!(
            stale_profile_patterns(&test_profiles()),
            [
                String::from("--user-data-dir=/Users/alice/.peitho/chrome-profile-slides"),
                String::from("--user-data-dir=/Users/alice/.peitho/chrome-profile-presenter"),
            ]
        );
    }

    #[test]
    fn prepare_profile_dirs_creates_both_role_profiles() {
        let temp = tempfile::tempdir().expect("temp dir");
        let profiles = ChromeProfiles {
            slides: temp.path().join("slides"),
            presenter: temp.path().join("presenter"),
        };

        assert!(prepare_profile_dirs(Some(&profiles)));
        assert!(profiles.slides.is_dir());
        assert!(profiles.presenter.is_dir());
    }

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
                    .chain(
                        command
                            .args
                            .iter()
                            .map(|arg| arg.to_string_lossy().to_string()),
                    )
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>();

        println!("{}", rendered.join("\n"));
        assert!(rendered[0].contains("--user-data-dir=/Users/alice/.peitho/chrome-profile-slides"));
        assert!(rendered[0].contains("--remote-debugging-port=0"));
        assert!(
            rendered[1].contains("--user-data-dir=/Users/alice/.peitho/chrome-profile-presenter")
        );
        assert!(rendered[1].contains("--remote-debugging-port=0"));
        assert!(!rendered[1].contains("--window-size=1200,800"));
    }

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
}
