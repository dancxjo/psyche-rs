use assert_cmd::Command;

#[test]
fn show_help() {
    Command::cargo_bin("distilld")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn test_flag_exits() {
    Command::cargo_bin("distilld")
        .unwrap()
        .arg("--test")
        .assert()
        .success()
        .stdout("ok\n");
}
