use crate::{Mouth, SpeechStream, args::Args};
use reqwest::Client;
use std::sync::Arc;

/// Build the mouth motor and speech stream.
pub async fn build_mouth(args: &Args) -> Result<(Arc<Mouth>, Arc<SpeechStream>), reqwest::Error> {
    let mouth_http = Client::builder().pool_max_idle_per_host(10).build()?;

    let mouth = Arc::new(Mouth::new(
        mouth_http,
        args.tts_url.clone(),
        args.language_id.clone(),
    ));
    let audio_rx = mouth.subscribe();
    let text_rx = mouth.subscribe_text();
    let segment_rx = mouth.subscribe_segments();
    let stream = Arc::new(SpeechStream::new(audio_rx, text_rx, segment_rx));
    Ok((mouth, stream))
}
