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
            args: vec![
                OsString::from(format!("--app={url}")),
                OsString::from("--start-fullscreen"),
            ],
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
