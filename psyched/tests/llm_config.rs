use tempfile::tempdir;

#[tokio::test]
async fn load_first_llm_picks_first_provider() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("llm.toml");
    tokio::fs::write(
        &path,
        "[[llm]]\nprovider = \"ollama\"\nbase_url = \"http://1.2.3.4:11434\"\nmodels = [\"x\", \"y\"]\n\n[[llm]]\nprovider = \"ollama\"\nbase_url = \"http://5.6.7.8:11434\"\nmodels = [\"z\"]\n",
    )
        .await
        .unwrap();

    let (_reg, prof) = psyched::llm_config::load_first_llm(&path).await.unwrap();
    assert_eq!(prof.provider, "ollama");
    assert_eq!(prof.model, "x");
}
