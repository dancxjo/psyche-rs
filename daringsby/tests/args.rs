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
