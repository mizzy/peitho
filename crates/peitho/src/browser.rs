use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Command,
};

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
    pub chrome_profile_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
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
            let profile_dir = env
                .chrome_profile_dir
                .as_deref()
                .expect("guarded by is_some");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, path::PathBuf};

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

    #[test]
    fn macos_falls_back_to_open_when_google_chrome_is_absent() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Macos,
            mac_google_chrome_available: false,
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
                OsString::from("--start-fullscreen")
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
                OsString::from("--start-fullscreen")
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
            .chain(
                command
                    .args
                    .iter()
                    .map(|arg| arg.to_string_lossy().to_string()),
            )
            .collect::<Vec<_>>()
            .join(" ");
        println!("{rendered}");

        assert_eq!(
            rendered,
            "open -na Google Chrome --args --user-data-dir=/Users/alice/.peitho/chrome-profile --no-first-run --no-default-browser-check --app=http://127.0.0.1:8000/present.html --start-fullscreen"
        );
    }

    #[test]
    fn unsupported_platform_returns_no_command() {
        let env = BrowserEnvironment {
            platform: BrowserPlatform::Other,
            mac_google_chrome_available: false,
            linux_browser: None,
            chrome_profile_dir: None,
        };

        assert!(plan_browser_command("http://127.0.0.1:9000/present.html", &env).is_none());
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
