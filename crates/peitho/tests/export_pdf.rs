use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

mod util;
use util::test_chrome_path;

#[test]
#[ignore]
// Chromeを実行するE2Eテスト。cargo test -- --ignored で明示的に実行。
fn export_pdf_writes_nonempty_pdf() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping export_pdf_writes_nonempty_pdf: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("out.pdf");
    fs::write(
        &deck,
        "---\naspect_ratio: 16:9\nresolution: 1920x1080\n---\n# Exported PDF\n\n<!-- speaker secret -->\n",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .args(["export", "pdf"])
        .arg(&deck)
        .args(["-o"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("exported 1 slide(s)"));

    let bytes = fs::read(&out).unwrap();
    assert!(bytes.len() > 4);
    assert_eq!(&bytes[..5], b"%PDF-");
}

#[test]
#[ignore]
// Chromeを実行するE2Eテスト。cargo test -- --ignored で明示的に実行。
fn export_pdf_flattens_gradient_backgrounds_to_images() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping export_pdf_flattens_gradient_backgrounds_to_images: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let css_dir = dir.path().join("css");
    let out = dir.path().join("out.pdf");
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(
        css_dir.join("gradient.css"),
        r#".peitho-slide {
  color: #111827;
  background-image:
    radial-gradient(circle at 16% 24%, rgba(255, 255, 255, 0.86) 0 2px, transparent 3px),
    radial-gradient(circle at 78% 32%, rgba(255, 214, 102, 0.72) 0 3px, transparent 4px),
    linear-gradient(135deg, #0f172a 0%, #2563eb 44%, #f8fafc 100%);
  background-position: 0 0, 20px 12px, 0 0;
  background-repeat: repeat, repeat, no-repeat;
  background-size: 84px 84px, 128px 128px, 100% 100%;
}
"#,
    )
    .unwrap();
    fs::write(
        &deck,
        "---\naspect_ratio: 16:9\nresolution: 1920x1080\n---\n# Vector Text\n\nGradient PDF export\n",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .args(["export", "pdf"])
        .arg(&deck)
        .args(["-o"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("exported 1 slide(s)"));

    let bytes = fs::read(&out).unwrap();
    assert!(bytes.len() > 4);
    assert_eq!(&bytes[..5], b"%PDF-");
    assert!(!pdf_bytes_contain(&bytes, b"/Shading"));
    assert!(
        pdf_bytes_contain(&bytes, b"/Subtype /Image")
            || pdf_bytes_contain(&bytes, b"/Subtype/Image")
    );
    assert!(pdf_bytes_contain(&bytes, b"/Font"));
}

#[test]
#[ignore]
// Chromeを実行するE2Eテスト。cargo test -- --ignored で明示的に実行。
fn export_pdf_flattens_box_shadows_without_luminosity_smask_to_images() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping export_pdf_flattens_box_shadows_without_luminosity_smask_to_images: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let css_dir = dir.path().join("css");
    let out = dir.path().join("out.pdf");
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(
        css_dir.join("shadow.css"),
        r#".peitho-slide {
  background: #f8fafc;
  color: #111827;
}
.peitho-slide h1 {
  display: inline-block;
  padding: 24px 48px;
  border-radius: 28px;
  background: white;
  box-shadow: rgba(0, 0, 0, 0.45) 0px 18px 48px 0px;
}
"#,
    )
    .unwrap();
    fs::write(
        &deck,
        "---\naspect_ratio: 16:9\nresolution: 1920x1080\n---\n# Vector Text\n\nBox shadow PDF export\n",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .args(["export", "pdf"])
        .arg(&deck)
        .args(["-o"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("exported 1 slide(s)"));

    let bytes = fs::read(&out).unwrap();
    assert!(bytes.len() > 4);
    assert_eq!(&bytes[..5], b"%PDF-");
    assert!(!pdf_bytes_contain(&bytes, b"/Luminosity"));
    assert!(
        pdf_bytes_contain(&bytes, b"/Subtype /Image")
            || pdf_bytes_contain(&bytes, b"/Subtype/Image")
    );
    assert!(pdf_bytes_contain(&bytes, b"/Font"));
}

#[test]
#[ignore]
// Chromeを実行するE2Eテスト。cargo test -- --ignored で明示的に実行。
fn export_pdf_flattens_inset_box_shadows_without_luminosity_smask_to_images() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping export_pdf_flattens_inset_box_shadows_without_luminosity_smask_to_images: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let css_dir = dir.path().join("css");
    let out = dir.path().join("out.pdf");
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(
        css_dir.join("shadow.css"),
        r#".peitho-slide {
  background: #f8fafc;
  color: #111827;
}
.peitho-slide h1 {
  display: inline-block;
  padding: 24px 48px;
  border-radius: 28px;
  background: white;
  box-shadow: inset rgba(0, 0, 0, 0.45) 0px 6px 24px 0px;
}
"#,
    )
    .unwrap();
    fs::write(
        &deck,
        "---\naspect_ratio: 16:9\nresolution: 1920x1080\n---\n# Vector Text\n\nBox shadow PDF export\n",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .args(["export", "pdf"])
        .arg(&deck)
        .args(["-o"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("exported 1 slide(s)"));

    let bytes = fs::read(&out).unwrap();
    assert!(bytes.len() > 4);
    assert_eq!(&bytes[..5], b"%PDF-");
    assert!(!pdf_bytes_contain(&bytes, b"/Luminosity"));
    assert!(
        pdf_bytes_contain(&bytes, b"/Subtype /Image")
            || pdf_bytes_contain(&bytes, b"/Subtype/Image")
    );
    assert!(pdf_bytes_contain(&bytes, b"/Font"));
}

fn pdf_bytes_contain(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}
