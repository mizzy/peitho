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
