use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use crate::displays::{PresentationLayout, SavedWindowBounds, WindowPlacement};

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
    pub program: OsString,
    pub args: Vec<OsString>,
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

fn push_placement_args(args: &mut Vec<OsString>, placement: WindowPlacement) {
    match placement {
        WindowPlacement::Fullscreen { x, y } => {
            args.push(OsString::from(format!("--window-position={x},{y}")));
            args.push(OsString::from("--start-fullscreen"));
        }
        WindowPlacement::Windowed {
            x,
            y,
            width,
            height,
        } => {
            args.push(OsString::from(format!("--window-position={x},{y}")));
            args.push(OsString::from(format!("--window-size={width},{height}")));
        }
        WindowPlacement::Restored => {}
    }
}

fn chrome_slides_args(
    profile_dir: &Path,
    url: &str,
    placement: Option<WindowPlacement>,
) -> Vec<OsString> {
    let mut args = chrome_base_args(profile_dir, url);
    match placement {
        Some(placement) => push_placement_args(&mut args, placement),
        None => args.push(OsString::from("--start-fullscreen")),
    }
    args
}

fn chrome_presenter_args(
    profile_dir: &Path,
    url: &str,
    placement: WindowPlacement,
) -> Vec<OsString> {
    let mut args = chrome_base_args(profile_dir, url);
    push_placement_args(&mut args, placement);
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
            if let Some(layout) = env.layout.filter(|_| !request.no_presenter) {
                return vec![
                    macos_chrome_command(chrome_slides_args(
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
            vec![macos_chrome_command(chrome_slides_args(
                &profiles.slides,
                request.slides_url,
                None,
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

fn linux_browser_commands(
    request: &BrowserOpenRequest<'_>,
    env: &BrowserEnvironment,
) -> Vec<BrowserCommand> {
    let Some(program) = env.linux_browser.as_deref() else {
        return vec![BrowserCommand {
            program: OsString::from("xdg-open"),
            args: vec![OsString::from(request.slides_url)],
        }];
    };
    let Some(profiles) = env.chrome_profiles.as_ref() else {
        return vec![BrowserCommand {
            program: OsString::from("xdg-open"),
            args: vec![OsString::from(request.slides_url)],
        }];
    };

    if let Some(layout) = env.layout.filter(|_| !request.no_presenter) {
        return vec![
            BrowserCommand {
                program: program.to_owned(),
                args: chrome_slides_args(&profiles.slides, request.slides_url, Some(layout.slides)),
            },
            BrowserCommand {
                program: program.to_owned(),
                args: chrome_presenter_args(
                    &profiles.presenter,
                    request.presenter_url,
                    layout.presenter,
                ),
            },
        ];
    }

    vec![BrowserCommand {
        program: program.to_owned(),
        args: chrome_slides_args(&profiles.slides, request.slides_url, None),
    }]
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
                "warning: failed to prepare Chrome profile at {}: {err}",
                profile.display()
            );
            return false;
        }
    }
    true
}

pub fn open_browser_with_request(
    request: BrowserOpenRequest<'_>,
    layout: Option<PresentationLayout>,
) {
    let mut env = current_environment();
    env.layout = layout;
    if !prepare_profile_dirs(env.chrome_profiles.as_ref()) {
        env.chrome_profiles = None;
    }
    if let Some(profiles) = env.chrome_profiles.as_ref() {
        terminate_stale_profile_instances(profiles);
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
                OsString::from("--start-fullscreen"),
            ]
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
                OsString::from("--start-fullscreen"),
            ]
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
            .contains(&OsString::from("--start-fullscreen")));
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
        assert!(commands[0]
            .args
            .contains(&OsString::from("--start-fullscreen")));
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
        assert!(commands[0]
            .args
            .contains(&OsString::from("--start-fullscreen")));
        assert!(!commands[0]
            .args
            .iter()
            .any(|arg| arg == "--window-size=1200,800"));
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
        assert!(rendered[0].contains("--window-position=-1055,0"));
        assert!(rendered[0].contains("--start-fullscreen"));
        assert!(
            rendered[1].contains("--user-data-dir=/Users/alice/.peitho/chrome-profile-presenter")
        );
        assert!(rendered[1].contains("--window-position=156,91"));
        assert!(rendered[1].contains("--start-fullscreen"));
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
