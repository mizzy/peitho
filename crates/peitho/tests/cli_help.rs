use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn help_mentions_build_subcommand() {
    Command::cargo_bin("peitho")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("build"));
}

#[test]
fn lint_help_mentions_input_argument() {
    Command::cargo_bin("peitho")
        .unwrap()
        .args(["lint", "--help"])
        .assert()
        .success()
        .stdout(contains("Usage: peitho lint [INPUT]"));
}

#[test]
fn new_command_is_listed_in_help() {
    let assert = Command::cargo_bin("peitho")
        .unwrap()
        .arg("--help")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let lists_new_subcommand = stdout.lines().any(|line| {
        line.strip_prefix("  new")
            .and_then(|rest| rest.chars().next())
            .is_some_and(char::is_whitespace)
    });
    assert!(lists_new_subcommand, "actual stdout: {stdout}");
}
