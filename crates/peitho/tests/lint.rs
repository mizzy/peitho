use std::{fs, path::Path};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

mod util;
use util::{test_chrome_path, workspace_root};

const ELLIPSIS_TITLE_CSS: &str =
    ".slot-title { display: block; line-height: 1.2; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }\n";
const OVERLONG_TITLE: &str = "This deliberately overlong presentation title is truncated by the layout with an ellipsis instead of overflowing the title slot";
const DECORATION_LAYOUT: &str = r#"<section class="peitho-slide crn-section">
  <div class="section-mark" aria-hidden="true">{child}</div>
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body"><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure class="code"><slot name="code" accepts="code" arity="0..1"></slot></figure>
  <footer class="footnotes"><slot name="footnotes" accepts="blocks" arity="0..1"></slot></footer>
</section>"#;

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

fn write_layout(dir: &Path, name: &str, layout: &str) {
    let layouts_dir = dir.join("layouts");
    fs::create_dir_all(&layouts_dir).unwrap();
    fs::write(layouts_dir.join(name), layout).unwrap();
}

fn decoration_layout(child: &str) -> String {
    DECORATION_LAYOUT.replace("{child}", child)
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
    write_default_theme(dir.path(), ".body { overflow: visible; }\n");
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
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the slide box vertically by",
        ))
        .stdout(predicate::str::contains(
            "has text at 22.5pt, below the recommended 24pt:",
        ))
        .stdout(predicate::str::contains("checked 1 slide(s): 2 warning(s)"));
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
fn lint_reports_ellipsis_truncation_as_a_note_without_warning() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_ellipsis_truncation_as_a_note_without_warning: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(dir.path(), ELLIPSIS_TITLE_CSS);
    fs::write(&deck, format!("# {OVERLONG_TITLE}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "note: slide 1 text in the `title` slot is truncated with an ellipsis (",
        ))
        .stdout(predicate::str::contains("px hidden)"))
        .stdout(predicate::str::contains(
            "help: the layout CSS truncates this text with text-overflow; shorten the text or widen the slot if the cut is unintended",
        ))
        .stdout(predicate::str::contains("content overflows").not())
        .stdout(predicate::str::contains("has text at").not())
        .stdout(predicate::str::contains("checked 1 slide(s): no warnings"));
}

#[test]
#[ignore]
fn lint_reports_waived_small_text_as_a_note_and_text_below_the_floor_as_a_warning() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_waived_small_text_as_a_note_and_text_below_the_floor_as_a_warning: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        ".slot-body { font-size: 14pt; --peitho-lint-min-font-size: 12pt; }\n\
         [data-slide-key=\"tiny\"] .slot-body { font-size: 10pt; }\n",
    );
    fs::write(
        &deck,
        "# Allowed\n\nSource: annual report\n\n---\n\n<!-- {\"key\":\"tiny\"} -->\n# Too small\n\nFine print\n",
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
            "note: slide 1 has text at 14pt, allowed by --peitho-lint-min-font-size: 12pt: \"Source: annual report\"",
        ))
        .stdout(predicate::str::contains(
            "warning: slide 2 has text at 10pt, below the layout's --peitho-lint-min-font-size of 12pt: \"Fine print\"",
        ))
        .stdout(predicate::str::contains("below the recommended 24pt").not())
        .stdout(predicate::str::contains("checked 2 slide(s): 1 warning(s)"));
}

#[test]
#[ignore]
fn lint_rejects_an_invalid_font_size_waiver_value() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_rejects_an_invalid_font_size_waiver_value: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        ".slot-body { --peitho-lint-min-font-size: none; }\n",
    );
    fs::write(&deck, "# Title\n\nBody text\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "slide 1 has an invalid --peitho-lint-min-font-size value `none`",
        ))
        .stderr(predicate::str::contains(
            "use a length in pt or px, such as 12pt, or 0 to accept any size",
        ));
}

#[test]
#[ignore]
fn lint_reports_real_overflow_alongside_ellipsis_truncation_note() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_real_overflow_alongside_ellipsis_truncation_note: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(dir.path(), ELLIPSIS_TITLE_CSS);
    let bullets = twelve_bullets();
    fs::write(&deck, format!("# {OVERLONG_TITLE}\n\n{bullets}\n")).unwrap();

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
            "note: slide 1 text in the `title` slot is truncated with an ellipsis (",
        ))
        .stdout(predicate::str::contains("content overflows the `title` slot horizontally").not())
        .stdout(predicate::str::contains("checked 1 slide(s): 2 warning(s)"));
}

#[test]
#[ignore]
fn lint_keeps_inline_block_title_clip_as_overflow_warning() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_keeps_inline_block_title_clip_as_overflow_warning: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        ".peitho-slide h1 { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }\n",
    );
    fs::write(&deck, format!("# {OVERLONG_TITLE}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `title` slot horizontally by",
        ))
        .stdout(predicate::str::contains("note:").not());
}

#[test]
#[ignore]
fn lint_keeps_atomic_inline_child_clip_as_overflow_warning() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_keeps_atomic_inline_child_clip_as_overflow_warning: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        ".slot-body p { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; } .slot-body code { display: inline-block; }\n",
    );
    let long_code = "x".repeat(300);
    fs::write(&deck, format!("# Atomic\n\n`{long_code}`\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot horizontally by",
        ))
        .stdout(predicate::str::contains("note:").not());
}

#[test]
#[ignore]
fn lint_reports_linked_title_truncation_as_a_note_without_slide_box_warning() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_linked_title_truncation_as_a_note_without_slide_box_warning: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(dir.path(), ELLIPSIS_TITLE_CSS);
    fs::write(
        &deck,
        format!("# [{OVERLONG_TITLE}](https://example.com/)\n"),
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "note: slide 1 text in the `title` slot is truncated with an ellipsis (",
        ))
        .stdout(predicate::str::contains("overflows the slide box").not())
        .stdout(predicate::str::contains("content overflows").not())
        .stdout(predicate::str::contains("checked 1 slide(s): no warnings"));
}

#[test]
#[ignore]
fn lint_reports_positioned_escape_from_clipping_wrapper_as_slide_box_overflow() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_positioned_escape_from_clipping_wrapper_as_slide_box_overflow: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layouts_dir = dir.path().join("layouts");
    fs::create_dir_all(&layouts_dir).unwrap();
    fs::write(
        layouts_dir.join("title-body-code.html"),
        r#"<section class="peitho-slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body">
    <div class="badge"></div>
    <slot name="body" accepts="blocks" arity="0..*"></slot>
  </div>
  <figure class="code">
    <slot name="code" accepts="code" arity="0..1"></slot>
  </figure>
  <footer class="footnotes"><slot name="footnotes" accepts="blocks" arity="0..1"></slot></footer>
</section>"#,
    )
    .unwrap();
    write_default_theme(
        dir.path(),
        ".badge { position: absolute; left: -400px; top: 100px; width: 300px; height: 80px; background: red; }\n",
    );
    fs::write(&deck, "# Badge\n\nBody text\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the slide box horizontally by 400px",
        ));
}

#[test]
#[ignore]
fn lint_ignores_pseudo_element_overflow_inside_decoration_clipper() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_ignores_pseudo_element_overflow_inside_decoration_clipper: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_layout(dir.path(), "section.html", &decoration_layout(""));
    write_default_theme(
        dir.path(),
        r#".crn-section { position: relative; }
.section-mark { position: absolute; inset: 0; overflow: hidden; }
.section-mark::before {
  content: "03";
  position: absolute;
  right: -24px;
  bottom: -130px;
  font-size: 560px;
  line-height: 1;
}
"#,
    );
    fs::write(&deck, "# Section\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .success()
        .stdout(predicate::str::contains("warning:").not())
        .stdout(predicate::str::contains("checked 1 slide(s): no warnings"));
}

#[test]
#[ignore]
fn lint_ignores_start_side_bleed_inside_decoration_clipper() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_ignores_start_side_bleed_inside_decoration_clipper: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_layout(
        dir.path(),
        "section.html",
        &decoration_layout(r#"<div class="numeral"></div>"#),
    );
    write_default_theme(
        dir.path(),
        r#".crn-section { position: relative; }
.section-mark { position: absolute; inset: 0; overflow: hidden; }
.numeral {
  position: absolute;
  left: -24px;
  top: -130px;
  width: 600px;
  height: 600px;
  background: #000;
}
"#,
    );
    fs::write(&deck, "# Section\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .success()
        .stdout(predicate::str::contains("warning:").not())
        .stdout(predicate::str::contains("checked 1 slide(s): no warnings"));
}

#[test]
#[ignore]
fn lint_reports_overflow_from_slot_bearing_clipper() {
    let Some(chrome) = test_chrome_path() else {
        println!("skipping lint_reports_overflow_from_slot_bearing_clipper: Chrome not found");
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_layout(
        dir.path(),
        "section.html",
        r#"<section class="peitho-slide crn-section">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body-clipper">
    <slot name="body" accepts="blocks" arity="0..*"></slot>
  </div>
  <figure class="code"><slot name="code" accepts="code" arity="0..1"></slot></figure>
  <footer class="footnotes"><slot name="footnotes" accepts="blocks" arity="0..1"></slot></footer>
</section>"#,
    );
    write_default_theme(
        dir.path(),
        ".body-clipper { flex: none; height: 80px; overflow: hidden; }\n\
         .slot-body { font-size: 32px; }\n",
    );
    fs::write(
        &deck,
        "# Clipped body\n\nFirst line.\n\nSecond line.\n\nThird line.\n\nFourth line.\n",
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
            "warning: slide 1 content overflows the `body` slot vertically by",
        ));
}

#[test]
#[ignore]
fn lint_reports_decoration_clipper_box_that_escapes_slide() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_decoration_clipper_box_that_escapes_slide: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_layout(dir.path(), "section.html", &decoration_layout(""));
    write_default_theme(
        dir.path(),
        r#".crn-section { position: relative; }
.section-mark {
  position: absolute;
  top: 0;
  bottom: 0;
  right: -200px;
  width: 400px;
  overflow: hidden;
  background: #000;
}
"#,
    );
    fs::write(&deck, "# Section\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the slide box horizontally",
        ));
}

#[test]
#[ignore]
fn lint_reports_overflow_from_scrollable_slot_free_clipper() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_overflow_from_scrollable_slot_free_clipper: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_layout(dir.path(), "section.html", &decoration_layout(""));
    write_default_theme(
        dir.path(),
        r#".crn-section { position: relative; }
.section-mark { position: absolute; inset: 0; overflow: auto; }
.section-mark::before {
  content: "03";
  position: absolute;
  right: -24px;
  bottom: -130px;
  font-size: 560px;
  line-height: 1;
}
"#,
    );
    fs::write(&deck, "# Section\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows a container",
        ));
}

#[test]
#[ignore]
fn lint_reports_visible_axis_bleed_from_decoration_clipper() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_visible_axis_bleed_from_decoration_clipper: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_layout(
        dir.path(),
        "section.html",
        &decoration_layout(r#"<div class="numeral"></div>"#),
    );
    write_default_theme(
        dir.path(),
        r#".crn-section { position: relative; }
.section-mark {
  position: absolute;
  inset: 0;
  overflow-x: clip;
  overflow-y: visible;
}
.numeral {
  position: absolute;
  left: 0;
  top: -130px;
  width: 100px;
  height: 100px;
  background: #000;
}
"#,
    );
    fs::write(&deck, "# Section\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the slide box vertically by 130px",
        ));
}

#[test]
#[ignore]
fn lint_reports_decoration_painted_through_overflow_clip_margin() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_decoration_painted_through_overflow_clip_margin: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_layout(
        dir.path(),
        "section.html",
        &decoration_layout(r#"<div class="numeral"></div>"#),
    );
    write_default_theme(
        dir.path(),
        r#".crn-section { position: relative; }
.section-mark {
  position: absolute;
  inset: 0;
  overflow: clip;
  overflow-clip-margin: 200px;
}
.numeral {
  position: absolute;
  left: -150px;
  top: -150px;
  width: 400px;
  height: 400px;
  background: #000;
}
"#,
    );
    fs::write(&deck, "# Section\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the slide box horizontally by 150px",
        ));
}

#[test]
#[ignore]
fn lint_reports_positioned_escape_from_static_decoration_clipper() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_positioned_escape_from_static_decoration_clipper: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_layout(
        dir.path(),
        "section.html",
        &decoration_layout(r#"<div class="escape"></div>"#),
    );
    write_default_theme(
        dir.path(),
        r#".crn-section { position: relative; }
.section-mark { flex: none; height: 100px; overflow: hidden; }
.escape {
  position: absolute;
  left: -400px;
  top: 100px;
  width: 300px;
  height: 80px;
  background: #000;
}
"#,
    );
    fs::write(&deck, "# Section\n").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the slide box horizontally by 400px",
        ));
}

#[test]
#[ignore]
fn lint_reports_start_side_clip_inside_body_as_slide_box_overflow() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_reports_start_side_clip_inside_body_as_slide_box_overflow: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(dir.path(), ".slot-body p { margin-left: -400px; }\n");
    fs::write(
        &deck,
        "# Pulled left\n\nThis paragraph is pulled out of the body slot and clipped.\n",
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
            "warning: slide 1 content overflows the slide box horizontally by 328px",
        ));
}

#[test]
#[ignore]
fn lint_keeps_wrapper_clip_around_block_child_as_overflow_warning() {
    let Some(chrome) = test_chrome_path() else {
        println!(
            "skipping lint_keeps_wrapper_clip_around_block_child_as_overflow_warning: Chrome not found"
        );
        return;
    };
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_default_theme(
        dir.path(),
        ".body { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }\n",
    );
    let long_word = "x".repeat(320);
    fs::write(&deck, format!("# Wrapper clip\n\n{long_word}\n")).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .env("PEITHO_CHROME_PATH", chrome)
        .arg("lint")
        .arg(&deck)
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "warning: slide 1 content overflows the `body` slot horizontally by",
        ))
        .stdout(predicate::str::contains("note:").not());
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
