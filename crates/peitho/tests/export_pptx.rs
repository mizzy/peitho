use std::{
    env, fs,
    io::{Cursor, Read},
    path::PathBuf,
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;
use zip::ZipArchive;

#[test]
#[ignore]
// Chromeを実行するE2Eテスト。cargo test -- --ignored で明示的に実行。
fn export_pptx_writes_editable_zip_with_text_image_and_notes() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping export_pptx_writes_editable_zip_with_text_image_and_notes: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("out.pptx");
    fs::create_dir_all(dir.path().join("layouts")).unwrap();
    fs::create_dir_all(dir.path().join("css")).unwrap();
    fs::create_dir_all(dir.path().join("images")).unwrap();
    fs::write(
        dir.path().join("layouts/image.html"),
        r#"<section class="peitho-slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body"><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure class="hero"><slot name="hero" accepts="image" arity="1"></slot></figure>
</section>"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("css/base.css"),
        r#".peitho-slide { width: var(--peitho-canvas-width, 1280px); height: var(--peitho-canvas-height, 720px); position: relative; background: rgb(248, 248, 248); font-family: Arial, sans-serif; }
h1 { position: absolute; left: 96px; top: 72px; margin: 0; font-size: 48px; color: rgb(17, 34, 51); }
.body { position: absolute; left: 96px; top: 168px; width: 560px; font-size: 24px; color: rgb(30, 41, 59); }
.hero img { position: absolute; left: 760px; top: 160px; width: 320px; height: 180px; }
"#,
    )
    .unwrap();
    fs::write(dir.path().join("images/pixel.png"), PNG_1X1).unwrap();
    fs::write(
        &deck,
        "---\naspect_ratio: 16:9\nlayouts: layouts\ncss: css\n---\n# PPTX Export\n\nEditable body text.\n\n![Architecture](images/pixel.png)\n\n<!-- speaker note for pptx -->\n",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .args(["export", "pptx"])
        .arg(&deck)
        .args(["-o"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("exported 1 slide(s)"));

    let bytes = fs::read(&out).unwrap();
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("[Content_Types].xml").is_ok());
    assert!(zip.by_name("ppt/slides/slide1.xml").is_ok());
    assert!(zip.by_name("ppt/notesSlides/notesSlide1.xml").is_ok());
    assert!(zip.by_name("ppt/media/image1.png").is_ok());

    let slide = read_zip_text(&mut zip, "ppt/slides/slide1.xml");
    assert!(slide.contains("<a:t>PPTX Export</a:t>"));
    assert!(slide.contains("<a:t>Editable body text.</a:t>"));
    assert!(slide.contains(r#"descr="Architecture""#));

    let notes = read_zip_text(&mut zip, "ppt/notesSlides/notesSlide1.xml");
    assert!(notes.contains("speaker note for pptx"));

    let media = read_zip_bytes(&mut zip, "ppt/media/image1.png");
    assert_eq!(media, PNG_1X1);
}

fn test_chrome_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("PEITHO_CHROME_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
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

fn read_zip_text(zip: &mut ZipArchive<Cursor<Vec<u8>>>, path: &str) -> String {
    let mut file = zip.by_name(path).unwrap();
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap();
    content
}

fn read_zip_bytes(zip: &mut ZipArchive<Cursor<Vec<u8>>>, path: &str) -> Vec<u8> {
    let mut file = zip.by_name(path).unwrap();
    let mut content = Vec::new();
    file.read_to_end(&mut content).unwrap();
    content
}

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];
