use std::{
    env,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use miette::IntoDiagnostic;
use peitho::{
    browser::{self, BrowserPlatform},
    displays,
};
use serde_json::Value;

use crate::asset_resolution::{self, AssetKey, Provenance};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorReport {
    pub(crate) checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorCheck {
    pub(crate) category: DoctorCategory,
    pub(crate) name: &'static str,
    pub(crate) status: DoctorStatus,
    pub(crate) message: String,
    pub(crate) help: Option<String>,
    pub(crate) details: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorCategory {
    Chrome,
    Displays,
    EmbeddedShells,
    Assets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorStatus {
    Pass,
    Warn,
    Fail,
}

pub(crate) struct DoctorEnv {
    pub(crate) chrome_lookup: crate::ChromeLookupEnv,
    pub(crate) home: Option<OsString>,
    pub(crate) platform: BrowserPlatform,
    pub(crate) display_json: Option<String>,
}

impl DoctorEnv {
    pub(crate) fn from_process_env() -> Self {
        let path_dirs = env::var_os("PATH")
            .map(|path| env::split_paths(&path).collect())
            .unwrap_or_default();
        let platform = if cfg!(target_os = "macos") {
            BrowserPlatform::Macos
        } else if cfg!(target_os = "linux") {
            BrowserPlatform::Linux
        } else {
            BrowserPlatform::Other
        };
        let display_json = (platform == BrowserPlatform::Macos)
            .then(displays::detect_nsscreen_json)
            .flatten();

        Self {
            chrome_lookup: crate::ChromeLookupEnv {
                env_path: env::var_os("PEITHO_CHROME_PATH").map(PathBuf::from),
                mac_chrome: PathBuf::from(
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                ),
                path_dirs,
            },
            home: env::var_os("HOME"),
            platform,
            display_json,
        }
    }
}

pub(crate) fn run_doctor(deck: Option<&Path>, env: &DoctorEnv) -> DoctorReport {
    let mut checks = Vec::new();
    push_chrome_checks(&mut checks, env);
    push_display_checks(&mut checks, env);
    push_embedded_shell_checks(&mut checks);
    if let Some(deck) = deck {
        push_asset_checks(&mut checks, deck);
    }
    DoctorReport { checks }
}

pub(crate) fn dispatch(
    input: Option<PathBuf>,
    json: bool,
    env: &DoctorEnv,
    writer: &mut dyn Write,
    is_terminal: bool,
) -> miette::Result<i32> {
    let report = run_doctor(input.as_deref(), env);
    if json {
        print_json(&report, writer)?;
    } else {
        print_human(&report, writer, is_terminal)?;
    }
    Ok(exit_code_for(&report))
}

pub(crate) fn print_human<W: Write>(
    report: &DoctorReport,
    mut writer: W,
    is_terminal: bool,
) -> miette::Result<()> {
    let mut current_category = None;
    for check in &report.checks {
        if current_category != Some(check.category) {
            if current_category.is_some() {
                writeln!(writer).into_diagnostic()?;
            }
            writeln!(writer, "{}", check.category.human_name()).into_diagnostic()?;
            current_category = Some(check.category);
        }
        writeln!(
            writer,
            "  {} {}: {}",
            status_glyph(check.status, is_terminal),
            check.name,
            check.message
        )
        .into_diagnostic()?;
        if let Some(help) = &check.help {
            writeln!(writer, "      help: {help}").into_diagnostic()?;
        }
    }

    let summary = summary_for(report);
    writeln!(writer).into_diagnostic()?;
    writeln!(
        writer,
        "{} passed, {} warned, {} failed",
        summary.pass, summary.warn, summary.fail
    )
    .into_diagnostic()
}

pub(crate) fn print_json<W: Write>(report: &DoctorReport, mut writer: W) -> miette::Result<()> {
    let payload = JsonReport {
        checks: report
            .checks
            .iter()
            .map(|check| JsonCheck {
                category: check.category.json_name(),
                name: check.name,
                status: check.status.json_name(),
                message: &check.message,
                help: check.help.as_deref(),
                details: check.details.as_ref(),
            })
            .collect(),
        summary: summary_for(report),
    };
    serde_json::to_writer_pretty(&mut writer, &payload).into_diagnostic()?;
    writeln!(writer).into_diagnostic()
}

pub(crate) fn exit_code_for(report: &DoctorReport) -> i32 {
    if report
        .checks
        .iter()
        .any(|check| check.status == DoctorStatus::Fail)
    {
        2
    } else {
        0
    }
}

fn push_chrome_checks(checks: &mut Vec<DoctorCheck>, env: &DoctorEnv) {
    match crate::locate_chrome_with_env(&env.chrome_lookup) {
        Ok(path) => checks.push(DoctorCheck {
            category: DoctorCategory::Chrome,
            name: "binary-discovered",
            status: DoctorStatus::Pass,
            message: path.display().to_string(),
            help: None,
            details: Some(serde_json::json!({ "path": path.display().to_string() })),
        }),
        Err(_) => {
            checks.push(DoctorCheck {
                category: DoctorCategory::Chrome,
                name: "binary-discovered",
                status: DoctorStatus::Fail,
                message: chrome_missing_message(env),
                help: Some(
                    "install Google Chrome or Chromium, or set PEITHO_CHROME_PATH=<absolute-path>"
                        .to_owned(),
                ),
                details: None,
            });
        }
    }

    let Some(profiles) = browser::chrome_profiles_from_home(env.home.clone()) else {
        push_home_unavailable_warning(checks);
        return;
    };
    let root = profiles
        .slides
        .parent()
        .expect("chrome profiles path always has a parent");
    if !root.is_absolute() {
        push_home_unavailable_warning(checks);
        return;
    }

    let root_exists = match root.try_exists() {
        Ok(exists) => exists,
        Err(err) => {
            checks.push(DoctorCheck {
                category: DoctorCategory::Chrome,
                name: "profiles-writable",
                status: DoctorStatus::Fail,
                message: format!("{} ({err})", root.display()),
                help: Some("make HOME writable".to_owned()),
                details: Some(serde_json::json!({ "path": root.display().to_string() })),
            });
            return;
        }
    };

    if root_exists {
        match fs::metadata(root) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                checks.push(DoctorCheck {
                    category: DoctorCategory::Chrome,
                    name: "profiles-writable",
                    status: DoctorStatus::Fail,
                    message: format!("{} is not a directory", root.display()),
                    help: Some("remove the file or set HOME to a writable directory".to_owned()),
                    details: Some(serde_json::json!({ "path": root.display().to_string() })),
                });
                return;
            }
            Err(err) => {
                checks.push(DoctorCheck {
                    category: DoctorCategory::Chrome,
                    name: "profiles-writable",
                    status: DoctorStatus::Fail,
                    message: format!("{} ({err})", root.display()),
                    help: Some("make the Chrome profiles root readable".to_owned()),
                    details: Some(serde_json::json!({ "path": root.display().to_string() })),
                });
                return;
            }
        }
        if let Err(err) = probe_writable(root) {
            checks.push(DoctorCheck {
                category: DoctorCategory::Chrome,
                name: "profiles-writable",
                status: DoctorStatus::Fail,
                message: format!("{} ({err})", root.display()),
                help: Some("make the Chrome profiles root writable".to_owned()),
                details: Some(serde_json::json!({ "path": root.display().to_string() })),
            });
            return;
        }
        checks.push(DoctorCheck {
            category: DoctorCategory::Chrome,
            name: "profiles-writable",
            status: DoctorStatus::Pass,
            message: root.display().to_string(),
            help: None,
            details: Some(serde_json::json!({ "path": root.display().to_string() })),
        });
        return;
    }

    if let Err(err) = probe_create_remove_dir(root) {
        checks.push(DoctorCheck {
            category: DoctorCategory::Chrome,
            name: "profiles-writable",
            status: DoctorStatus::Fail,
            message: format!("{} ({err})", root.display()),
            help: Some("make HOME writable".to_owned()),
            details: Some(serde_json::json!({ "path": root.display().to_string() })),
        });
        return;
    }
    checks.push(DoctorCheck {
        category: DoctorCategory::Chrome,
        name: "profiles-writable",
        status: DoctorStatus::Pass,
        message: format!("{} will be created on first present", root.display()),
        help: None,
        details: Some(serde_json::json!({ "path": root.display().to_string() })),
    });
}

fn probe_writable(dir: &Path) -> std::io::Result<()> {
    let probe = dir.join(format!(".doctor-write-test-{}", std::process::id()));
    fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe)
        .and_then(|mut file| file.write_all(b"ok"))?;
    let _ = fs::remove_file(probe);
    Ok(())
}

fn probe_create_remove_dir(dir: &Path) -> std::io::Result<()> {
    match fs::create_dir(dir) {
        Ok(()) => {
            let _ = fs::remove_dir(dir);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => probe_writable(dir),
        Err(err) => Err(err),
    }
}

fn push_home_unavailable_warning(checks: &mut Vec<DoctorCheck>) {
    checks.push(DoctorCheck {
        category: DoctorCategory::Chrome,
        name: "profiles-writable",
        status: DoctorStatus::Warn,
        message: "HOME is unset or not absolute; persistent Chrome profiles cannot be prepared"
            .to_owned(),
        help: Some(
            "set HOME to an absolute writable directory before running present mode".to_owned(),
        ),
        details: None,
    });
}

fn push_display_checks(checks: &mut Vec<DoctorCheck>, env: &DoctorEnv) {
    if env.platform != BrowserPlatform::Macos {
        checks.push(DoctorCheck {
            category: DoctorCategory::Displays,
            name: "enumeration",
            status: DoctorStatus::Warn,
            message: "display enumeration is not implemented on this platform (see Issue #22)"
                .to_owned(),
            help: None,
            details: None,
        });
        return;
    }

    let Some(json) = env.display_json.as_deref() else {
        checks.push(DoctorCheck {
            category: DoctorCategory::Displays,
            name: "enumeration",
            status: DoctorStatus::Warn,
            message: "display enumeration failed".to_owned(),
            help: Some(
                "osascript may not be installed, or NSScreen access may be denied".to_owned(),
            ),
            details: None,
        });
        return;
    };

    let displays = match displays::parse_nsscreen_json(json) {
        Ok(displays) => displays,
        Err(err) => {
            checks.push(DoctorCheck {
                category: DoctorCategory::Displays,
                name: "enumeration",
                status: DoctorStatus::Warn,
                message: format!("display enumeration failed: {err}"),
                help: Some(
                    "osascript may not be installed, or NSScreen access may be denied".to_owned(),
                ),
                details: None,
            });
            return;
        }
    };

    let count = displays.len();
    let (status, help) = match count {
        0 => (
            DoctorStatus::Warn,
            Some(
                "no displays are visible; peitho may be running headlessly or NSScreen access may be denied"
                    .to_owned(),
            ),
        ),
        1 => (
            DoctorStatus::Warn,
            Some("connect an external display for presenter mode".to_owned()),
        ),
        _ => (DoctorStatus::Pass, None),
    };
    checks.push(DoctorCheck {
        category: DoctorCategory::Displays,
        name: "enumeration",
        status,
        message: format!("{count} display(s) found"),
        help,
        details: Some(serde_json::Value::Array(
            displays
                .iter()
                .map(|display| {
                    serde_json::json!({
                        "primary": display.primary,
                        "x": display.x,
                        "y": display.y,
                        "width": display.width,
                        "height": display.height,
                    })
                })
                .collect(),
        )),
    });
}

fn push_embedded_shell_checks(checks: &mut Vec<DoctorCheck>) {
    checks.push(shell_check("present-shell", crate::BUILTIN_SHELL_JS));
    checks.push(shell_check("preview-shell", crate::BUILTIN_PREVIEW_JS));
}

fn shell_check(name: &'static str, source: &str) -> DoctorCheck {
    let bytes = source.len();
    let sha256_prefix = crate::short_sha256_hex(source.as_bytes(), 12);
    DoctorCheck {
        category: DoctorCategory::EmbeddedShells,
        name,
        status: DoctorStatus::Pass,
        message: format!("{bytes} bytes (sha256: {sha256_prefix})"),
        help: None,
        details: Some(serde_json::json!({
            "bytes": bytes,
            "sha256_prefix": sha256_prefix,
        })),
    }
}

fn push_asset_checks(checks: &mut Vec<DoctorCheck>, deck: &Path) {
    let markdown = match fs::read_to_string(deck) {
        Ok(markdown) => markdown,
        Err(err) => {
            checks.push(DoctorCheck {
                category: DoctorCategory::Assets,
                name: "deck",
                status: DoctorStatus::Fail,
                message: format!("failed to read {}: {err}", deck.display()),
                help: Some("pass the deck path explicitly if it lives elsewhere".to_owned()),
                details: Some(serde_json::json!({ "path": deck.display().to_string() })),
            });
            return;
        }
    };
    let frontmatter = match peitho_core::parse_frontmatter(&markdown) {
        Ok(frontmatter) => frontmatter,
        Err(err) => {
            checks.push(asset_error_check(
                "frontmatter",
                err,
                Some(serde_json::json!({ "path": deck.display().to_string() })),
            ));
            return;
        }
    };

    for key in AssetKey::ALL {
        match asset_resolution::resolve_asset(deck, &frontmatter, key) {
            Ok(provenance) => checks.push(asset_pass_check(key, provenance)),
            Err(err) => {
                checks.push(asset_error_check(key.as_str(), err, None));
            }
        }
    }
}

fn chrome_missing_message(env: &DoctorEnv) -> String {
    match &env.chrome_lookup.env_path {
        Some(path) if !path.as_os_str().is_empty() => {
            format!("Chrome not found at PEITHO_CHROME_PATH={}", path.display())
        }
        _ => "Chrome not found".to_owned(),
    }
}

fn asset_pass_check(key: AssetKey, provenance: Provenance) -> DoctorCheck {
    let provenance_name = provenance.kind();
    let path = provenance.path().map(|path| path.display().to_string());
    let message = match &path {
        Some(path) => format!("{provenance_name} {path}"),
        None => provenance_name.to_owned(),
    };
    DoctorCheck {
        category: DoctorCategory::Assets,
        name: key.as_str(),
        status: DoctorStatus::Pass,
        message,
        help: None,
        details: Some(serde_json::json!({
            "provenance": provenance_name,
            "path": path,
        })),
    }
}

fn asset_error_check(
    name: &'static str,
    err: peitho_core::error::BuildError,
    details: Option<serde_json::Value>,
) -> DoctorCheck {
    let help = (!err.help.is_empty()).then_some(err.help);
    DoctorCheck {
        category: DoctorCategory::Assets,
        name,
        status: DoctorStatus::Fail,
        message: err.message,
        help,
        details,
    }
}

fn status_glyph(status: DoctorStatus, is_terminal: bool) -> String {
    let (glyph, color) = match status {
        DoctorStatus::Pass => ("✓", "32"),
        DoctorStatus::Warn => ("⚠", "33"),
        DoctorStatus::Fail => ("✗", "31"),
    };
    if is_terminal {
        format!("\x1b[{color}m{glyph}\x1b[0m")
    } else {
        glyph.to_owned()
    }
}

fn summary_for(report: &DoctorReport) -> Summary {
    let mut summary = Summary {
        pass: 0,
        warn: 0,
        fail: 0,
    };
    for check in &report.checks {
        match check.status {
            DoctorStatus::Pass => summary.pass += 1,
            DoctorStatus::Warn => summary.warn += 1,
            DoctorStatus::Fail => summary.fail += 1,
        }
    }
    summary
}

impl DoctorCategory {
    fn human_name(self) -> String {
        self.json_name().replace('-', " ")
    }

    fn json_name(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Displays => "displays",
            Self::EmbeddedShells => "embedded-shells",
            Self::Assets => "assets",
        }
    }
}

impl DoctorStatus {
    fn json_name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(serde::Serialize)]
struct JsonReport<'a> {
    checks: Vec<JsonCheck<'a>>,
    summary: Summary,
}

#[derive(serde::Serialize)]
struct JsonCheck<'a> {
    category: &'static str,
    name: &'static str,
    status: &'static str,
    message: &'a str,
    help: Option<&'a str>,
    details: Option<&'a Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
struct Summary {
    pass: usize,
    warn: usize,
    fail: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::{fs, path::Path};

    fn write_file(path: &Path) {
        fs::write(path, "fake").unwrap();
    }

    fn chrome_lookup_with_existing(path: PathBuf) -> crate::ChromeLookupEnv {
        crate::ChromeLookupEnv {
            env_path: Some(path),
            mac_chrome: PathBuf::from("/missing/mac/chrome"),
            path_dirs: Vec::new(),
        }
    }

    fn chrome_lookup_missing() -> crate::ChromeLookupEnv {
        crate::ChromeLookupEnv {
            env_path: None,
            mac_chrome: PathBuf::from("/missing/mac/chrome"),
            path_dirs: Vec::new(),
        }
    }

    fn two_display_json() -> String {
        r#"[
          {"x":0,"y":0,"width":1512,"height":982},
          {"x":1512,"y":0,"width":1920,"height":1080}
        ]"#
        .to_owned()
    }

    fn passing_env(dir: &Path) -> DoctorEnv {
        let chrome = dir.join("chrome");
        let home = dir.join("home");
        write_file(&chrome);
        fs::create_dir_all(&home).unwrap();
        DoctorEnv {
            chrome_lookup: chrome_lookup_with_existing(chrome),
            home: Some(home.into_os_string()),
            platform: BrowserPlatform::Macos,
            display_json: Some(two_display_json()),
        }
    }

    fn find_check<'a>(
        report: &'a DoctorReport,
        category: DoctorCategory,
        name: &str,
    ) -> &'a DoctorCheck {
        report
            .checks
            .iter()
            .find(|check| check.category == category && check.name == name)
            .unwrap_or_else(|| panic!("missing check {category:?}/{name} in {report:#?}"))
    }

    #[test]
    fn cli_dispatch_calls_run_doctor_and_exits_with_code() {
        let cli = crate::Cli::parse_from(["peitho", "doctor", "--json"]);
        let crate::Command::Doctor { input, json } = cli.command else {
            panic!("expected doctor command");
        };
        assert_eq!(input, None);
        assert!(json);

        let passing_dir = tempfile::tempdir().unwrap();
        let env = passing_env(passing_dir.path());
        let mut output = Vec::new();
        let code = dispatch(input.clone(), json, &env, &mut output, false).unwrap();
        assert_eq!(code, 0);
        let text = String::from_utf8(output).unwrap();
        assert!(text.starts_with("{\n  \"checks\""));

        let failing_dir = tempfile::tempdir().unwrap();
        let failing_env = DoctorEnv {
            chrome_lookup: chrome_lookup_missing(),
            ..passing_env(failing_dir.path())
        };
        let mut output = Vec::new();
        let code = dispatch(input, json, &failing_env, &mut output, false).unwrap();
        assert_eq!(code, 2);
    }

    #[test]
    fn chrome_binary_discovered_reports_pass() {
        let dir = tempfile::tempdir().unwrap();
        let env = passing_env(dir.path());

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Chrome, "binary-discovered");
        assert_eq!(check.status, DoctorStatus::Pass);
        assert!(check.message.contains("chrome"));
    }

    #[test]
    fn chrome_binary_missing_reports_fail_with_help() {
        let dir = tempfile::tempdir().unwrap();
        let env = DoctorEnv {
            chrome_lookup: chrome_lookup_missing(),
            ..passing_env(dir.path())
        };

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Chrome, "binary-discovered");
        assert_eq!(check.status, DoctorStatus::Fail);
        assert!(check
            .help
            .as_deref()
            .is_some_and(|help| help.contains("PEITHO_CHROME_PATH")));
    }

    #[test]
    fn chrome_binary_missing_omits_empty_env_path_in_message() {
        let dir = tempfile::tempdir().unwrap();
        let env = DoctorEnv {
            chrome_lookup: crate::ChromeLookupEnv {
                env_path: Some(PathBuf::from("")),
                mac_chrome: PathBuf::from("/missing/mac/chrome"),
                path_dirs: Vec::new(),
            },
            ..passing_env(dir.path())
        };

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Chrome, "binary-discovered");
        assert_eq!(check.status, DoctorStatus::Fail);
        assert_eq!(check.message, "Chrome not found");
    }

    #[test]
    fn chrome_profiles_writable_reports_pass_for_existing_home() {
        let dir = tempfile::tempdir().unwrap();
        let env = passing_env(dir.path());

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Chrome, "profiles-writable");
        assert_eq!(check.status, DoctorStatus::Pass);
        assert!(check.message.contains(".peitho"));
    }

    #[test]
    fn chrome_profiles_reports_pass_without_creating_peitho_root_when_home_is_writable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("home").join(".peitho");
        let env = passing_env(dir.path());
        assert!(!root.exists());

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Chrome, "profiles-writable");
        assert_eq!(check.status, DoctorStatus::Pass);
        assert!(check.message.contains("will be created on first present"));
        assert!(!root.exists());
        let home_entries = fs::read_dir(root.parent().unwrap())
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(home_entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn chrome_profiles_probe_ignores_cleanup_error_and_reports_pass() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let probe = dir
            .path()
            .join(format!(".doctor-write-test-{}", std::process::id()));
        fs::write(&probe, "old").unwrap();
        let original = fs::metadata(dir.path()).unwrap().permissions();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();

        let result = probe_writable(dir.path());

        fs::set_permissions(dir.path(), original).unwrap();
        assert!(result.is_ok());
        assert!(probe.exists());
    }

    #[test]
    fn chrome_profiles_probe_treats_already_exists_as_pass_via_directory_probe() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("home").join(".peitho");
        let env = passing_env(dir.path());
        fs::create_dir(&root).unwrap();

        assert!(probe_create_remove_dir(&root).is_ok());
        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Chrome, "profiles-writable");
        assert_eq!(check.status, DoctorStatus::Pass);
        assert_eq!(check.message, root.display().to_string());
        assert!(root.exists());
    }

    #[test]
    fn chrome_profiles_reports_warn_when_home_unset() {
        let dir = tempfile::tempdir().unwrap();
        let env = DoctorEnv {
            home: None,
            ..passing_env(dir.path())
        };

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Chrome, "profiles-writable");
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.message.contains("HOME"));
    }

    #[test]
    fn chrome_profiles_reports_warn_when_home_is_empty_or_relative() {
        let dir = tempfile::tempdir().unwrap();
        for home in [OsString::from(""), OsString::from("relative-home")] {
            let env = DoctorEnv {
                home: Some(home),
                ..passing_env(dir.path())
            };

            let report = run_doctor(None, &env);

            let check = find_check(&report, DoctorCategory::Chrome, "profiles-writable");
            assert_eq!(check.status, DoctorStatus::Warn);
            assert!(check.message.contains("HOME"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn chrome_profiles_reports_fail_when_home_readonly() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let original = fs::metadata(&home).unwrap().permissions();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o555)).unwrap();
        assert!(!home.join(".peitho").exists());
        let env = DoctorEnv {
            home: Some(home.clone().into_os_string()),
            ..passing_env(dir.path())
        };

        let report = run_doctor(None, &env);

        fs::set_permissions(&home, original).unwrap();
        let check = find_check(&report, DoctorCategory::Chrome, "profiles-writable");
        assert_eq!(check.status, DoctorStatus::Fail);
    }

    #[test]
    fn displays_enumeration_reports_pass_when_two_or_more_displays() {
        let dir = tempfile::tempdir().unwrap();
        let env = passing_env(dir.path());

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Displays, "enumeration");
        assert_eq!(check.status, DoctorStatus::Pass);
        assert!(check.message.contains("2 display(s) found"));
        let details = check.details.as_ref().unwrap().as_array().unwrap();
        assert_eq!(details.len(), 2);
        assert_eq!(details[0]["primary"], true);
        assert_eq!(details[1]["x"], 1512);
    }

    #[test]
    fn displays_enumeration_reports_warn_when_zero_displays_returned() {
        let dir = tempfile::tempdir().unwrap();
        let env = DoctorEnv {
            display_json: Some("[]".to_owned()),
            ..passing_env(dir.path())
        };

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Displays, "enumeration");
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.message.contains("0 display(s) found"));
        assert!(check
            .help
            .as_deref()
            .is_some_and(|help| help.contains("headlessly")));
    }

    #[test]
    fn displays_enumeration_reports_warn_when_provider_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let env = DoctorEnv {
            display_json: None,
            ..passing_env(dir.path())
        };

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Displays, "enumeration");
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check
            .help
            .as_deref()
            .is_some_and(|help| help.contains("NSScreen")));
    }

    #[test]
    fn displays_reports_warn_on_non_macos() {
        let dir = tempfile::tempdir().unwrap();
        let env = DoctorEnv {
            platform: BrowserPlatform::Linux,
            ..passing_env(dir.path())
        };

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::Displays, "enumeration");
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.message.contains("Issue #22"));
    }

    #[test]
    fn embedded_shells_report_present_shell_with_byte_count() {
        let dir = tempfile::tempdir().unwrap();
        let env = passing_env(dir.path());

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::EmbeddedShells, "present-shell");
        assert_eq!(check.status, DoctorStatus::Pass);
        assert!(check
            .message
            .contains(&crate::BUILTIN_SHELL_JS.len().to_string()));
    }

    #[test]
    fn embedded_shells_report_preview_shell_with_byte_count() {
        let dir = tempfile::tempdir().unwrap();
        let env = passing_env(dir.path());

        let report = run_doctor(None, &env);

        let check = find_check(&report, DoctorCategory::EmbeddedShells, "preview-shell");
        assert_eq!(check.status, DoctorStatus::Pass);
        assert!(check
            .message
            .contains(&crate::BUILTIN_PREVIEW_JS.len().to_string()));
    }

    #[test]
    fn assets_report_skipped_when_no_deck_path() {
        let dir = tempfile::tempdir().unwrap();
        let env = passing_env(dir.path());

        let report = run_doctor(None, &env);

        let categories = report
            .checks
            .iter()
            .map(|check| check.category)
            .collect::<Vec<_>>();
        assert_eq!(
            categories,
            vec![
                DoctorCategory::Chrome,
                DoctorCategory::Chrome,
                DoctorCategory::Displays,
                DoctorCategory::EmbeddedShells,
                DoctorCategory::EmbeddedShells,
            ]
        );
        assert!(!report
            .checks
            .iter()
            .any(|check| check.category == DoctorCategory::Assets));
    }

    #[test]
    fn assets_report_provenance_for_each_asset() {
        let dir = tempfile::tempdir().unwrap();
        let env = passing_env(dir.path());
        let deck = dir.path().join("deck.md");
        fs::write(&deck, "# Intro\n").unwrap();
        fs::create_dir_all(dir.path().join("layouts")).unwrap();

        let report = run_doctor(Some(&deck), &env);

        let layouts = find_check(&report, DoctorCategory::Assets, "layouts");
        let css = find_check(&report, DoctorCategory::Assets, "css");
        let syntaxes = find_check(&report, DoctorCategory::Assets, "syntaxes");
        let fonts = find_check(&report, DoctorCategory::Assets, "fonts");
        assert_eq!(layouts.status, DoctorStatus::Pass);
        assert!(layouts.message.contains("deck-adjacent"));
        assert!(css.message.contains("built-in"));
        assert!(syntaxes.message.contains("built-in"));
        assert!(fonts.message.contains("built-in"));
    }

    #[test]
    fn assets_report_fail_for_missing_explicit_path() {
        let dir = tempfile::tempdir().unwrap();
        let env = DoctorEnv {
            chrome_lookup: chrome_lookup_missing(),
            ..passing_env(dir.path())
        };
        let deck = dir.path().join("deck.md");
        fs::write(
            &deck,
            "---\nlayouts: ./layouts\ncss: ./missing.css\nsyntaxes: ./syntaxes\nfonts: ./fonts\n---\n# Intro\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("layouts")).unwrap();
        fs::create_dir_all(dir.path().join("syntaxes")).unwrap();
        fs::create_dir_all(dir.path().join("fonts")).unwrap();

        let report = run_doctor(Some(&deck), &env);

        let categories = report
            .checks
            .iter()
            .map(|check| check.category)
            .collect::<Vec<_>>();
        assert_eq!(
            categories,
            vec![
                DoctorCategory::Chrome,
                DoctorCategory::Chrome,
                DoctorCategory::Displays,
                DoctorCategory::EmbeddedShells,
                DoctorCategory::EmbeddedShells,
                DoctorCategory::Assets,
                DoctorCategory::Assets,
                DoctorCategory::Assets,
                DoctorCategory::Assets,
            ]
        );
        let layouts = find_check(&report, DoctorCategory::Assets, "layouts");
        let css = find_check(&report, DoctorCategory::Assets, "css");
        let syntaxes = find_check(&report, DoctorCategory::Assets, "syntaxes");
        let fonts = find_check(&report, DoctorCategory::Assets, "fonts");
        assert_eq!(layouts.status, DoctorStatus::Pass);
        assert_eq!(css.status, DoctorStatus::Fail);
        assert_eq!(syntaxes.status, DoctorStatus::Pass);
        assert_eq!(fonts.status, DoctorStatus::Pass);
        assert!(css.message.contains("css path does not exist"));
        assert!(!css.message.contains("= help:"));
        assert!(css
            .help
            .as_deref()
            .is_some_and(|help| help.contains("frontmatter")));
    }

    #[test]
    fn assets_report_omits_help_when_build_error_help_is_empty() {
        let check = asset_error_check(
            "css",
            peitho_core::error::BuildError {
                kind: peitho_core::error::ErrorKind::Parse,
                line: None,
                message: "css path is invalid".to_owned(),
                help: String::new(),
                slide: None,
            },
            None,
        );

        assert_eq!(check.status, DoctorStatus::Fail);
        assert_eq!(check.message, "css path is invalid");
        assert_eq!(check.help, None);
    }

    #[test]
    fn run_doctor_exit_code_is_pass_when_all_pass() {
        let report = DoctorReport {
            checks: vec![DoctorCheck {
                category: DoctorCategory::Chrome,
                name: "binary-discovered",
                status: DoctorStatus::Pass,
                message: "ok".to_owned(),
                help: None,
                details: None,
            }],
        };

        assert_eq!(exit_code_for(&report), 0);
    }

    #[test]
    fn run_doctor_exit_code_is_fail_when_any_fail() {
        let report = DoctorReport {
            checks: vec![
                DoctorCheck {
                    category: DoctorCategory::Chrome,
                    name: "binary-discovered",
                    status: DoctorStatus::Pass,
                    message: "ok".to_owned(),
                    help: None,
                    details: None,
                },
                DoctorCheck {
                    category: DoctorCategory::Assets,
                    name: "css",
                    status: DoctorStatus::Fail,
                    message: "missing".to_owned(),
                    help: None,
                    details: None,
                },
            ],
        };

        assert_eq!(exit_code_for(&report), 2);
    }

    #[test]
    fn run_doctor_warn_does_not_fail_exit_code() {
        let report = DoctorReport {
            checks: vec![DoctorCheck {
                category: DoctorCategory::Displays,
                name: "enumeration",
                status: DoctorStatus::Warn,
                message: "not implemented".to_owned(),
                help: None,
                details: None,
            }],
        };

        assert_eq!(exit_code_for(&report), 0);
    }

    #[test]
    fn print_human_writes_category_headers_and_status_glyphs() {
        let report = DoctorReport {
            checks: vec![
                DoctorCheck {
                    category: DoctorCategory::Chrome,
                    name: "binary-discovered",
                    status: DoctorStatus::Pass,
                    message: "ok".to_owned(),
                    help: None,
                    details: None,
                },
                DoctorCheck {
                    category: DoctorCategory::Displays,
                    name: "enumeration",
                    status: DoctorStatus::Warn,
                    message: "single display".to_owned(),
                    help: Some("connect another display".to_owned()),
                    details: None,
                },
                DoctorCheck {
                    category: DoctorCategory::Assets,
                    name: "css",
                    status: DoctorStatus::Fail,
                    message: "missing".to_owned(),
                    help: None,
                    details: None,
                },
            ],
        };
        let mut output = Vec::new();

        print_human(&report, &mut output, false).unwrap();

        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("chrome\n"));
        assert!(text.contains("displays\n"));
        assert!(text.contains("assets\n"));
        assert!(text.contains("✓ binary-discovered: ok"));
        assert!(text.contains("⚠ enumeration: single display"));
        assert!(text.contains("✗ css: missing"));
        assert!(text.contains("1 passed, 1 warned, 1 failed"));
    }

    #[test]
    fn print_human_uses_no_color_when_not_tty() {
        let report = DoctorReport {
            checks: vec![DoctorCheck {
                category: DoctorCategory::Chrome,
                name: "binary-discovered",
                status: DoctorStatus::Pass,
                message: "ok".to_owned(),
                help: None,
                details: None,
            }],
        };
        let mut output = Vec::new();

        print_human(&report, &mut output, false).unwrap();

        let text = String::from_utf8(output).unwrap();
        assert!(!text.contains("\x1b["));
    }

    #[test]
    fn print_json_shape_is_stable_categories_names_and_details() {
        let report = DoctorReport {
            checks: vec![DoctorCheck {
                category: DoctorCategory::EmbeddedShells,
                name: "present-shell",
                status: DoctorStatus::Pass,
                message: "12 bytes".to_owned(),
                help: None,
                details: Some(serde_json::json!({"sha256_prefix": "abcdef123456"})),
            }],
        };
        let mut output = Vec::new();

        print_json(&report, &mut output).unwrap();

        let payload: Value = serde_json::from_slice(&output).unwrap();
        let check = payload["checks"][0].as_object().unwrap();
        let mut keys = check.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["category", "details", "help", "message", "name", "status"]
        );
        assert_eq!(check["category"], "embedded-shells");
        assert_eq!(check["name"], "present-shell");
        assert_eq!(check["help"], Value::Null);
        assert!(payload["summary"].get("pass").is_some());
        assert!(payload["summary"].get("warn").is_some());
        assert!(payload["summary"].get("fail").is_some());
    }
}
