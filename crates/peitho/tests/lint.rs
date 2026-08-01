use std::{fs, path::Path};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

mod util;
use util::{test_chrome_path, workspace_root};

fn write_default_theme(dir: &Path, overrides: &str) {
    let css_dir = dir.join("css");
    fs::create_dir_all(&css_dir).unwrap();
    fs::copy(
        workspace_root().join("themes/base.css"),
        css_dir.join("base.css"),
    )
    .unwrap();
    fs::write(css_dir.join("overrides.css"), overrides).unwrap();
}

fn twelve_bullets() -> String {
    (1..=12)
        .map(|index| format!("- Bullet {index}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn lint_chrome_lookup_failure_does_not_keep_workspace() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let missing_chrome = dir.path().join("missing-chrome");
    fs::write(&deck, "# Tiny\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", &missing_chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Chrome not found at PEITHO_CHROME_PATH",
        ))
        .stderr(predicate::str::contains("workspace kept at").not());
}

#[test]
#[ignore]
fn lint_reports_slide_vertical_overflow() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_reports_slide_vertical_overflow: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let paragraphs = (1..=80)
        .map(|index| {
            format!(
                "Paragraph {index}: this default-theme body text is intentionally tall enough to be clipped inside the slide body."
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    fs::write(&deck, format!("# Overflow\n\n{paragraphs}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains("warning: slide 1"))
        .stdout(predicate::str::contains("vertically"))
        .stdout(predicate::str::contains("px"))
        .stdout(predicate::str::contains(
            "has text at 22.5pt, below the recommended 24pt:",
        ))
        .stdout(predicate::str::contains("checked 1 slide(s): 3 warning(s)"));
}

#[test]
#[ignore]
fn lint_reports_clipped_body_and_accepts_healthy_deck() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_reports_clipped_body_and_accepts_healthy_deck: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let clipped_deck = dir.path().join("clipped.md");
    let bullets = twelve_bullets();
    fs::write(&clipped_deck, format!("# Clipped body\n\n{bullets}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", &chrome)
        .arg("lint")
        .arg(&clipped_deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot vertically by",
        ))
        .stdout(predicate::str::contains("px"))
        .stdout(predicate::str::contains("checked 1 slide(s): 2 warning(s)"));

    let healthy_deck = dir.path().join("healthy.md");
    fs::write(&healthy_deck, "# Healthy\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&healthy_deck)
        .assert()
        .success()
        .stdout(predicate::str::contains("checked 1 slide(s): no warnings"));
}

#[test]
#[ignore]
fn lint_detects_clipping_on_container_with_decorative_clip_path() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_detects_clipping_on_container_with_decorative_clip_path: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(dir.path(), ".body { clip-path: inset(0 round 12px); }\n");
    let bullets = twelve_bullets();
    fs::write(&deck, format!("# Clipped body\n\n{bullets}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot vertically by",
        ))
        .stdout(predicate::str::contains("px"))
        .stdout(predicate::str::contains("checked 1 slide(s): 2 warning(s)"));
}

#[test]
#[ignore]
fn lint_detects_overflow_clip() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_detects_overflow_clip: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        ".body { flex: none; height: 200px; overflow: clip; }\n",
    );
    let bullets = twelve_bullets();
    fs::write(&deck, format!("# Clipped body\n\n{bullets}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot vertically by",
        ))
        .stdout(predicate::str::contains("checked 1 slide(s): 2 warning(s)"));
}

#[test]
#[ignore]
fn lint_detects_vertical_overflow_when_clipping_container_width_collapses() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_detects_vertical_overflow_when_clipping_container_width_collapses: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        ".body { flex: none; width: 0; height: 200px; overflow: hidden; }\n",
    );
    let bullets = twelve_bullets();
    fs::write(&deck, format!("# Collapsed body\n\n{bullets}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot vertically by",
        ))
        .stdout(predicate::str::contains("content overflows the `body` slot horizontally").not())
        .stdout(predicate::str::contains("checked 1 slide(s):"));
}

#[test]
#[ignore]
fn lint_explains_overflow_in_scrollable_region() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_explains_overflow_in_scrollable_region: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        ".body { flex: none; height: 200px; overflow-y: auto; }\n",
    );
    let bullets = twelve_bullets();
    fs::write(&deck, format!("# Scrollable body\n\n{bullets}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot vertically by",
        ))
        .stdout(predicate::str::contains(
            "help: a scrollable region cannot be scrolled in a printed or projected deck, so content past the edge will not be seen",
        ));
}

#[test]
#[ignore]
fn lint_ignores_excess_on_visible_overflow() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_ignores_excess_on_visible_overflow: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        r#".body {
  flex: none;
  height: 100px;
  overflow: visible;
}

.slot-body {
  height: 240px;
  font-size: 32px;
}

.slot-body p {
  margin: 0;
}
"#,
    );
    fs::write(&deck, "# Visible overflow\n\nVisible content\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .success()
        .stdout(predicate::str::contains("content overflows").not())
        .stdout(predicate::str::contains("checked 1 slide(s): no warnings"));
}

#[test]
#[ignore]
fn lint_reports_both_axes_when_one_slot_clips_horizontally_and_vertically() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_both_axes_when_one_slot_clips_horizontally_and_vertically: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        r#"
.body {
  flex: none;
  width: 120px;
  height: 120px;
}

.slot-body {
  width: 240px;
  height: 240px;
  font-size: 32px;
}
"#,
    );
    fs::write(&deck, "# Both axes\n\nClipped content\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot horizontally by 120px",
        ))
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot vertically by 120px",
        ))
        .stdout(predicate::str::contains("checked 1 slide(s): 2 warning(s)"));
}

#[test]
#[ignore]
fn lint_leaves_slot_unnamed_when_clipping_container_wraps_multiple_slots() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_leaves_slot_unnamed_when_clipping_container_wraps_multiple_slots: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layouts_dir = dir.path().join("layouts");
    let css_dir = dir.path().join("css");
    fs::create_dir_all(&layouts_dir).unwrap();
    fs::create_dir_all(&css_dir).unwrap();
    fs::write(
        layouts_dir.join("ambiguous.html"),
        r#"<section class="ambiguous peitho-slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="slots">
    <slot name="left" accepts="blocks" arity="1"></slot>
    <slot name="right" accepts="blocks" arity="1"></slot>
  </div>
</section>"#,
    )
    .unwrap();
    fs::write(
        css_dir.join("base.css"),
        r#".peitho-slide {
  width: var(--peitho-canvas-width, 1280px);
  height: var(--peitho-canvas-height, 720px);
  box-sizing: border-box;
  overflow: hidden;
  font-size: 32px;
}

.slots {
  display: flex;
  width: 240px;
  height: 100px;
  overflow: hidden;
}

.slot-left {
  flex: none;
  width: 100px;
  height: 40px;
}

.slot-right {
  flex: none;
  width: 100px;
  height: 160px;
}

.slot-left p,
.slot-right p {
  margin: 0;
}
"#,
    )
    .unwrap();
    fs::write(
        &deck,
        r#"---
layouts: ./layouts
css: ./css
---
# Ambiguous slots

::: {slot=left}

Left

:::

::: {slot=right}

Right

:::
"#,
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows a container vertically by 60px",
        ))
        .stdout(predicate::str::contains("the `left` slot").not())
        .stdout(predicate::str::contains("the `right` slot").not())
        .stdout(predicate::str::contains("checked 1 slide(s): 1 warning(s)"));
}

#[test]
#[ignore]
fn lint_accepts_trivially_small_deck() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_accepts_trivially_small_deck: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "# Tiny\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .success()
        .stdout(predicate::str::contains("checked 1 slide(s): no warnings"));
}

#[test]
#[ignore]
fn lint_peitho_tour_has_no_overflow_warnings() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_peitho_tour_has_no_overflow_warnings: Chrome not found");
        return;
    };
    let deck = workspace_root().join("examples/peitho-tour/deck.md");

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(deck)
        .assert()
        .stdout(predicate::str::contains("content overflows").not())
        .stdout(predicate::str::contains("checked "));
}

#[test]
#[ignore]
fn lint_math_deck_has_no_overflow_warnings() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_math_deck_has_no_overflow_warnings: Chrome not found");
        return;
    };
    let deck = workspace_root().join("examples/math/deck.md");

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(deck)
        .assert()
        .stdout(predicate::str::contains("content overflows").not())
        .stdout(predicate::str::contains("checked "));
}
