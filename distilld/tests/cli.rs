use assert_cmd::Command;

#[test]
fn show_help() {
    Command::cargo_bin("distilld")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}
