use std::process::Command;

#[test]
fn split_sentences_splits_text() {
    let output = Command::new("bash")
        .arg("-c")
        .arg("source layka/memory-loop.sh; printf 'One. Two? Three!' | split_sentences")
        .output()
        .expect("failed to run bash");
    assert!(output.status.success());
    let out = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = out.trim().split('\n').collect();
    assert_eq!(lines, ["One.", "Two?", "Three!"]);
}
