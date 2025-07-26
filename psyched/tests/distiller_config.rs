use psyched::config;
use tempfile::tempdir;

#[tokio::test]
async fn load_config_parses_distillers() {
    let toml = r#"
        [[distillers]]
        name = "instant"
        input = "sensation/chat"
        output = "instant"
        prompt_template = "Summarize {{current}}"
    "#;
    let dir = tempdir().unwrap();
    let path = dir.path().join("psyche.toml");
    tokio::fs::write(&path, toml).await.unwrap();
    let cfg = config::load(&path).await.unwrap();
    assert_eq!(cfg.distillers.len(), 1);
    assert_eq!(cfg.distillers[0].name, "instant");
}
