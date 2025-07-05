use clap::Parser;
use daringsby::args::Args;

#[test]
fn quick_url_flag_overrides_default() {
    let args = Args::parse_from(["test", "--quick-url", "http://quick"]);
    assert_eq!(args.quick_url, "http://quick".to_string());
}

#[test]
fn default_quick_url_is_localhost() {
    let args = Args::parse_from(["test"]);
    assert_eq!(args.quick_url, "http://localhost:11434".to_string());
}

#[test]
fn voice_url_flag_overrides_default() {
    let args = Args::parse_from(["test", "--voice-url", "http://voice"]);
    assert_eq!(args.voice_url, "http://voice".to_string());
}

#[test]
fn default_voice_url_is_localhost() {
    let args = Args::parse_from(["test"]);
    assert_eq!(args.voice_url, "http://localhost:11434".to_string());
}

#[test]
fn default_voice_model_is_gemma3() {
    let args = Args::parse_from(["test"]);
    assert_eq!(args.voice_model, "gemma3:27b".to_string());
}
