use psyched::config;
use tempfile::tempdir;

#[tokio::test]
async fn load_spoken_config() {
    let toml = r#"
        [spoken]
        socket = "voice.sock"
        tts_url = "http://localhost:5002"
        speaker_id = "p1"
        language_id = ""
    "#;
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    tokio::fs::write(&path, toml).await.unwrap();
    let cfg = config::load(&path).await.unwrap();
    let spk = cfg.spoken.expect("spoken config");
    assert_eq!(spk.speaker_id, "p1");
    assert_eq!(spk.socket, "voice.sock");
}
