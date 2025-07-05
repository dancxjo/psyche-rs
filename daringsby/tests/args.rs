use clap::Parser;
use daringsby::args::Args;

#[test]
fn parse_multiple_base_urls_collects_all_values() {
    let args = Args::parse_from([
        "test",
        "--base-url",
        "http://one",
        "--base-url",
        "http://two",
    ]);
    assert_eq!(
        args.base_url,
        vec!["http://one".to_string(), "http://two".to_string()]
    );
}

#[test]
fn default_base_url_is_localhost() {
    let args = Args::parse_from(["test"]);
    assert_eq!(args.base_url, vec!["http://localhost:11434".to_string()]);
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

#[test]
fn llm_concurrency_parses_as_option() {
    let args = Args::parse_from(["test", "--llm-concurrency", "5"]);
    assert_eq!(args.llm_concurrency, Some(5));
}

#[test]
fn llm_concurrency_defaults_to_none() {
    let args = Args::parse_from(["test"]);
    assert!(args.llm_concurrency.is_none());
}
