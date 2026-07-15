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
