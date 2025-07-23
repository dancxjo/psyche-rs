use assert_cmd::Command;

#[test]
fn show_help() {
    Command::cargo_bin("distill")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}
