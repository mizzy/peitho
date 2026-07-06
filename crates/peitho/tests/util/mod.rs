use std::{env, path::PathBuf};

pub fn test_chrome_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("PEITHO_CHROME_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
        panic!(
            "PEITHO_CHROME_PATH is set but does not point to a file: {}",
            path.display()
        );
    }

    let mac_chrome = PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    if mac_chrome.is_file() {
        return Some(mac_chrome);
    }

    for program in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ] {
        if let Some(path) = find_in_path(program) {
            return Some(path);
        }
    }

    None
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(program);
        candidate.is_file().then_some(candidate)
    })
}
