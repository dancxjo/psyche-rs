use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tracing::{debug, trace};

/// Available Whisper models.
pub const MODELS: &[&str] = &[
    "tiny.en",
    "tiny",
    "base.en",
    "base",
    "small.en",
    "small",
    "medium.en",
    "medium",
    "large",
];

/// Prompt the user to select a model using [`inquire`].
pub fn prompt_model() -> anyhow::Result<String> {
    let sel = inquire::Select::new("Whisper model", MODELS.to_vec()).prompt()?;
    Ok(sel.to_string())
}

/// Download the given model into `dir` and return the saved path.
///
/// The base download URL can be overridden with the `WHISPER_MODEL_BASE_URL`
/// environment variable for testing.
pub async fn download(model: &str, dir: &Path) -> anyhow::Result<PathBuf> {
    let base = std::env::var("WHISPER_MODEL_BASE_URL")
        .unwrap_or_else(|_| "https://huggingface.co/ggerganov/whisper.cpp/resolve/main".into());
    let url = format!("{base}/ggml-{model}.bin");
    debug!(%url, "downloading model");
    tokio::fs::create_dir_all(dir).await?;
    let resp = reqwest::get(&url).await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let pb = ProgressBar::new(total);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(ProgressStyle::with_template(
        "{bar:40.cyan/blue} {bytes}/{total_bytes}",
    )?);
    let path = dir.join(format!("whisper-{model}.bin"));
    let mut file = tokio::fs::File::create(&path).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        if total > 0 {
            pb.inc(chunk.len() as u64);
        } else {
            pb.tick();
        }
    }
    file.flush().await?;
    pb.finish_and_clear();
    trace!(path = %path.display(), "download complete");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).await?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::Method;
    use httpmock::prelude::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn download_saves_file() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(Method::GET).path("/ggml-tiny.en.bin");
                then.status(200).body("ok");
            })
            .await;

        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("WHISPER_MODEL_BASE_URL", server.base_url());
        }
        let path = download("tiny.en", dir.path()).await.unwrap();
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "ok");
        unsafe {
            std::env::remove_var("WHISPER_MODEL_BASE_URL");
        }
    }
}
