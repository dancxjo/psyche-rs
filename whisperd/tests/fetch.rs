use assert_cmd::Command;
use httpmock::Method;
use httpmock::prelude::*;
use tempfile::tempdir;

#[test]
fn fetch_model_downloads_file() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/ggml-tiny.en.bin");
        then.status(200).body("ok");
    });

    let dir = tempdir().unwrap();
    unsafe {
        std::env::set_var("WHISPER_MODEL_BASE_URL", server.url(""));
    }

    Command::cargo_bin("whisperd")
        .unwrap()
        .args([
            "fetch-model",
            "--model",
            "tiny.en",
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    unsafe {
        std::env::remove_var("WHISPER_MODEL_BASE_URL");
    }
    let data = std::fs::read(dir.path().join("whisper-tiny.en.bin")).unwrap();
    assert_eq!(data, b"ok");
}
