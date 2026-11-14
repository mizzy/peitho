use std::fs;

use assert_cmd::Command;

#[test]
fn layouts_command_prints_builtin_layout_summary() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "# Intro\n\nBody\n").unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("layouts source: built-in"),
        "actual stdout: {stdout}"
    );
    assert!(
        stdout.contains("title-body-code"),
        "actual stdout: {stdout}"
    );
    assert!(
        stdout.contains("- title") && stdout.contains("accepts=inline arity=1"),
        "actual stdout: {stdout}"
    );
    assert!(
        stdout.contains("- footnotes") && stdout.contains("accepts=blocks arity=0..1"),
        "actual stdout: {stdout}"
    );
}

#[test]
fn layouts_command_json_prints_expected_shape() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "# Intro\n\nBody\n").unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--json")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["source"]["kind"], "built-in");
    assert!(json["source"].get("path").is_none());
    assert_eq!(json["layouts"][0]["name"], "title-body-code");
    assert_eq!(json["layouts"][0]["slots"][0]["name"], "body");
    assert!(json["layouts"][0]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .any(|slot| slot["name"] == "footnotes"
            && slot["accepts"] == "blocks"
            && slot["arity"] == "0..1"));
}

#[test]
fn layouts_explain_matching_slide_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "<!-- {\"key\":\"intro\"} -->\n# Intro\n\nBody\n").unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("intro")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("slide: intro (index 0)"),
        "actual stdout: {stdout}"
    );
    assert!(
        stdout.contains("dispatch: sole layout"),
        "actual stdout: {stdout}"
    );
    assert!(
        stdout.contains("result: title-body-code"),
        "actual stdout: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn layouts_explain_applies_code_images_transform() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layouts = dir.path().join("layouts");
    let command = dir.path().join("svg-command.sh");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        &command,
        "#!/bin/sh\ncat >/dev/null\nprintf '<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"></svg>'\n",
    )
    .unwrap();
    fs::write(
        layouts.join("code.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="code" accepts="code" arity="1"></slot></section>"#,
    )
    .unwrap();
    fs::write(
        layouts.join("image.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="image" accepts="image" arity="1"></slot></section>"#,
    )
    .unwrap();
    fs::write(
        &deck,
        format!(
            "---\ncode_images:\n  mermaid: /bin/sh {}\n---\n<!-- {{\"key\":\"diagram\"}} -->\n# Diagram\n\n```mermaid\ngraph TD\n```",
            command.display()
        ),
    )
    .unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("diagram")
        .arg("--json")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["dispatch"]["result"], "image");
}

#[test]
fn layouts_explain_unknown_slide_key_exits_two_with_help() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "<!-- {\"key\":\"intro\"} -->\n# Intro\n").unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("missing")
        .assert()
        .code(2);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("slide key 'missing' not found"),
        "actual stderr: {stderr}"
    );
    assert!(
        stderr.contains("help: known keys: intro"),
        "actual stderr: {stderr}"
    );
}

#[test]
fn layouts_explain_unknown_slide_key_with_json_emits_structured_error() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "<!-- {\"key\":\"intro\"} -->\n# Intro\n\nBody\n").unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("missing")
        .arg("--json")
        .assert()
        .code(2);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(json["error"], "slide-key-not-found");
    assert_eq!(json["key"], "missing");
    assert!(json["known_keys"]
        .as_array()
        .unwrap()
        .iter()
        .any(|key| key.as_str() == Some("intro")));
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("slide key 'missing' not found"));
}

#[test]
fn layouts_explain_unknown_explicit_layout_prints_trace_and_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(
        &deck,
        "<!-- {\"key\":\"bad\",\"layout\":\"missing\"} -->\n# Hi\n",
    )
    .unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("bad")
        .assert()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("result: unknown layout: missing"),
        "actual stdout: {stdout}"
    );

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("bad")
        .arg("--json")
        .assert()
        .code(1);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["dispatch"]["kind"], "explicit");
    assert_eq!(json["dispatch"]["result"]["kind"], "unknown-layout");
    assert_eq!(json["dispatch"]["result"]["layout"], "missing");
}

#[test]
fn layouts_explain_dispatch_failure_exits_one_and_prints_trace() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layouts = dir.path().join("layouts");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        layouts.join("cover.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
    )
    .unwrap();
    fs::write(
        layouts.join("statement.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot><slot name="body" accepts="blocks" arity="1..*"></slot></section>"#,
    )
    .unwrap();
    fs::write(
        &deck,
        "<!-- {\"key\":\"bad\"} -->\n# Intro\n\n```rust\nfn main() {}\n```\n",
    )
    .unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("bad")
        .assert()
        .code(1);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("layouts source: deck-adjacent"),
        "actual stdout: {stdout}"
    );
    assert!(
        stdout.contains("dispatch: structural match"),
        "actual stdout: {stdout}"
    );
    assert!(stdout.contains("rejected:"), "actual stdout: {stdout}");
    assert!(
        stdout.contains("result: no match"),
        "actual stdout: {stdout}"
    );
}

#[test]
fn layouts_explain_json_includes_reason_for_sole_layout_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layouts = dir.path().join("layouts");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        layouts.join("cover.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
    )
    .unwrap();
    fs::write(
        &deck,
        "<!-- {\"key\":\"bad\"} -->\n# Hi\n\n![alt](pic.png)\n",
    )
    .unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("bad")
        .arg("--json")
        .assert()
        .code(1);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["dispatch"]["kind"], "sole-layout");
    assert_eq!(json["dispatch"]["result"]["kind"], "no-match");
    assert_eq!(json["slide"]["key"], "bad");
    let reason = json["dispatch"]["result"]["reason"].as_str().unwrap();
    assert!(!reason.is_empty());
    assert!(reason.contains("image"), "actual reason: {reason}");
}

#[test]
fn layouts_explain_sole_layout_failure_prints_reason() {
    let dir = tempfile::tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let layouts = dir.path().join("layouts");
    fs::create_dir_all(&layouts).unwrap();
    fs::write(
        layouts.join("cover.html"),
        r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
    )
    .unwrap();
    fs::write(
        &deck,
        "<!-- {\"key\":\"hello\"} -->\n# Hello\n\n![alt](pic.png)\n",
    )
    .unwrap();

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("layouts")
        .arg(&deck)
        .arg("--explain")
        .arg("hello")
        .assert()
        .code(1);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("dispatch: sole layout"),
        "actual stdout: {stdout}"
    );
    assert!(
        stdout.contains("result: no match"),
        "actual stdout: {stdout}"
    );
    assert!(
        stdout.contains("reason: no slot in layout 'cover' accepts this image"),
        "actual stdout: {stdout}"
    );
}
