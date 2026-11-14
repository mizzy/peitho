use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn docs_command_smoke() {
    Command::cargo_bin("peitho")
        .unwrap()
        .arg("docs")
        .assert()
        .success()
        .stdout(contains("peitho docs <topic>"));

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["docs", "missing"])
        .assert()
        .failure()
        .stderr(contains("invalid value 'missing'"));
}
