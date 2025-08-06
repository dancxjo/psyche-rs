use std::process::Command;

#[test]
fn dispatch_invokes_function_script() {
    let output = Command::new("bash")
        .arg("-c")
        .arg("source layka/memory-loop.sh; export FUNCTIONS_DIR=layka/functions; dispatch_function '<function name=\"speak\">hello</function>'")
        .output()
        .expect("run bash");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("speak: hello"));
}
