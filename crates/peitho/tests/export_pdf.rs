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
// Chrome-backed E2E test; run explicitly with cargo test -- --ignored.
fn export_pdf_expands_public_reveal_steps_into_pages() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping export_pdf_expands_public_reveal_steps_into_pages: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("out.pdf");
    fs::write(
        &deck,
        r#"---
public_reveal: steps
---

# Stepped

::: {reveal}

- First item
- Second item

:::

---

# Plain
"#,
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
        .stdout(predicate::str::contains("exported 2 slide(s)"));

    let bytes = fs::read(&out).unwrap();
    assert!(bytes.len() > 4);
    assert_eq!(&bytes[..5], b"%PDF-");
    assert_eq!(pdf_page_object_count(&bytes), 4);
}

#[test]
#[ignore]
// Chrome-backed E2E test; run explicitly with cargo test -- --ignored.
fn export_pdf_reports_shadow_mounted_failure_without_writing_pdf() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping export_pdf_reports_shadow_mounted_failure_without_writing_pdf: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layouts = dir.path().join("layouts");
    let out = dir.path().join("out.pdf");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        layouts.join("duplicating.html"),
        r#"<section class="peitho-slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body"><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure class="code"><slot name="code" accepts="code" arity="0..1"></slot></figure>
  <footer class="footnotes"><slot name="footnotes" accepts="blocks" arity="0..1"></slot></footer>
  <script>
    (() => {
      const slide = document.currentScript.closest(".peitho-slide");
      const duplicate = slide.cloneNode(true);
      duplicate.querySelector("script").remove();
      document.body.appendChild(duplicate);
    })();
  </script>
</section>"#,
    )
    .unwrap();
    fs::write(&deck, "---\nlayouts: ./layouts\n---\n# Duplicated\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .args(["export", "pdf"])
        .arg(&deck)
        .args(["-o"])
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("PDF slide initialization failed"))
        .stderr(predicate::str::contains(
            "Unable to announce parsed slides: \"duplicated\" matched 2",
        ));

    assert!(!out.exists(), "failed export must not write a PDF");
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
// Chrome-backed E2E test; run explicitly with cargo test -- --ignored.
fn export_pdf_skips_hidden_box_shadows_without_emitting_images() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping export_pdf_skips_hidden_box_shadows_without_emitting_images: Chrome not found"
        );
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
  background: #fff;
  color: #111827;
}
.peitho-slide h1 {
  display: inline-block;
  visibility: hidden;
  padding: 24px 48px;
  background: #fff;
  box-shadow: rgba(0, 0, 0, 0.45) 0px 18px 48px 0px;
}
"#,
    )
    .unwrap();
    fs::write(
        &deck,
        "---\naspect_ratio: 16:9\nresolution: 1920x1080\n---\n# Hidden Shadow\n\nVisible text\n",
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
    assert_eq!(pdf_image_object_count(&bytes), 0);
    assert!(pdf_bytes_contain(&bytes, b"/Font"));
}

#[test]
#[ignore]
// Chrome-backed E2E test; run explicitly with cargo test -- --ignored.
fn export_pdf_skips_box_shadows_under_zero_opacity_ancestor() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping export_pdf_skips_box_shadows_under_zero_opacity_ancestor: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let css_dir = dir.path().join("css");
    let layouts_dir = dir.path().join("layouts");
    let out = dir.path().join("out.pdf");
    fs::create_dir_all(&css_dir).unwrap();
    fs::create_dir_all(&layouts_dir).unwrap();
    fs::write(
        css_dir.join("shadow.css"),
        r#".peitho-slide {
  background: #fff;
  color: #111827;
}
.transparent-shadow-parent {
  opacity: 0;
}
.shadow-child {
  width: 320px;
  height: 180px;
  background: #fff;
  box-shadow: rgba(0, 0, 0, 0.45) 0px 18px 48px 0px;
}
"#,
    )
    .unwrap();
    fs::write(
        layouts_dir.join("zero-opacity-shadow.html"),
        r#"<section class="peitho-slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="transparent-shadow-parent">
    <div class="shadow-child"></div>
  </div>
</section>"#,
    )
    .unwrap();
    fs::write(
        &deck,
        "---\naspect_ratio: 16:9\nresolution: 1920x1080\nlayouts: ./layouts\n---\n# Hidden Ancestor Shadow\n",
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
    assert_eq!(pdf_image_object_count(&bytes), 0);
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

fn pdf_page_object_count(bytes: &[u8]) -> usize {
    const PAGE_TYPE_TOKENS: [&[u8]; 2] = [b"/Type /Page", b"/Type/Page"];

    PAGE_TYPE_TOKENS
        .iter()
        .map(|token| {
            bytes
                .windows(token.len() + 1)
                .filter(|window| window.starts_with(token) && window[token.len()] != b's')
                .count()
        })
        .sum()
}

fn pdf_image_object_count(bytes: &[u8]) -> usize {
    const IMAGE_SUBTYPE_TOKENS: [&[u8]; 2] = [b"/Subtype /Image", b"/Subtype/Image"];

    IMAGE_SUBTYPE_TOKENS
        .iter()
        .map(|token| {
            bytes
                .windows(token.len())
                .filter(|window| *window == *token)
                .count()
        })
        .sum()
}
