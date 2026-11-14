use assert_cmd::Command;
use predicates::str::{contains, starts_with};

#[test]
fn docs_command_smoke() {
    Command::cargo_bin("peitho")
        .unwrap()
        .arg("docs")
        .assert()
        .success()
        .stdout(contains("peitho docs <topic>"));

    let topic = Command::cargo_bin("peitho")
        .unwrap()
        .args(["docs", "getting-started"])
        .assert()
        .success()
        .stdout(starts_with("# "));
    let topic_stdout = String::from_utf8(topic.get_output().stdout.clone()).unwrap();
    assert!(!topic_stdout.ends_with("\n\n"));

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["docs", "--all"])
        .assert()
        .success()
        .stdout(contains(topic_stdout));

    Command::cargo_bin("peitho")
        .unwrap()
        .args(["docs", "missing"])
        .assert()
        .failure()
        .stderr(contains("invalid value 'missing' for '[TOPIC]'"));
}
