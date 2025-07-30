use psyched::config;
use tempfile::tempdir;

#[tokio::test]
async fn load_sensor_model() {
    let toml = r#"
        [sensor.whisperd]
        whisper_model = "model.bin"
    "#;
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    tokio::fs::write(&path, toml).await.unwrap();
    let cfg = config::load(&path).await.unwrap();
    let sensor = cfg.sensor.get("whisperd").expect("sensor");
    assert_eq!(sensor.whisper_model.as_deref(), Some("model.bin"));
}
