use assert_cmd::Command;

#[test]
fn new_command_scaffolds_from_cli() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("starter");

    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("new")
        .arg(&target)
        .arg("--layouts")
        .arg("split")
        .arg("--theme")
        .arg("dark")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("deck.md"), "actual stdout: {stdout}");
    assert!(stdout.contains("peitho preview"), "actual stdout: {stdout}");
    assert!(target.join("deck.md").is_file());
    assert!(target.join("layouts/title-body-code.html").is_file());
    assert!(target.join("layouts/two-column.html").is_file());
    assert!(target.join("css/base.css").is_file());
    assert!(target.join(".gitignore").is_file());
}
