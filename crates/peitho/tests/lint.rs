use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

mod util;
use util::test_chrome_path;

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
            "checked 1 slide(s): 1 overflow warning(s)",
        ));
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
        .stdout(predicate::str::contains("checked 1 slide(s): no overflow"));
}
