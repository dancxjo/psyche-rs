use httpmock::prelude::*;
use psyche_rs::mouth::{AudioChunk, CoquiMouth, Mouth};
use tokio::sync::mpsc;

#[tokio::test]
async fn streams_sentence_chunks() -> anyhow::Result<()> {
    let server = MockServer::start_async().await;

    let first = server
        .mock_async(|when, then| {
            when.method(POST).path("/tts").json_body(serde_json::json!({
                "text": "Hello world. ",
                "speaker_id": "",
                "language_id": "",
            }));
            then.status(200).body("wav1");
        })
        .await;

    let second = server
        .mock_async(|when, then| {
            when.method(POST).path("/tts").json_body(serde_json::json!({
                "text": "How are you?",
                "speaker_id": "",
                "language_id": "",
            }));
            then.status(200).body("wav2");
        })
        .await;

    let (tx, mut rx) = mpsc::channel(8);
    let mouth = CoquiMouth {
        tx,
        endpoint: server.url(""),
        language_id: String::new(),
        speaker_id: String::new(),
    };

    mouth.say("Hello world. How are you?").await?;

    let mut chunks = Vec::new();
    while let Some(c) = rx.recv().await {
        let done = matches!(c, AudioChunk::Done);
        chunks.push(c);
        if done {
            break;
        }
    }

    assert_eq!(
        chunks,
        vec![
            AudioChunk::Data(b"wav1".to_vec()),
            AudioChunk::Data(b"wav2".to_vec()),
            AudioChunk::Done,
        ]
    );
    first.assert();
    second.assert();
    Ok(())
}
