use psyched::config;
use tempfile::tempdir;

#[tokio::test]
async fn load_config_parses_distillers() {
    let toml = r#"
        [wit.instant]
        input = "sensation/chat"
        output = "instant"
        prompt = "Summarize {{current}}"
    "#;
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    tokio::fs::write(&path, toml).await.unwrap();
    let cfg = config::load(&path).await.unwrap();
    assert_eq!(cfg.wit.len(), 1);
    assert!(cfg.wit.contains_key("instant"));
}
