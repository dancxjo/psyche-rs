use distill::{run, Config};
use httpmock::prelude::*;
use httpmock::Method;
use ollama_rs::Ollama;
use tokio::io::BufReader;

#[tokio::test]
async fn run_streams_output() {
    let server = MockServer::start_async().await;
    let body = concat!(
        "{\"created_at\":\"now\",\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"foo\"},\"done\":false}\n",
        "{\"created_at\":\"now\",\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"bar\"},\"done\":true}\n"
    );
    server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/chat");
            then.status(200)
                .header("content-type", "application/json")
                .body(body);
        })
        .await;

    let ollama = Ollama::try_new(&server.base_url()).unwrap();
    let cfg = Config {
        continuous: false,
        lines: 1,
        prompt: "Summarize: {{current}}".into(),
        model: "llama3".into(),
    };
    let input = BufReader::new("hi".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    assert_eq!(std::str::from_utf8(&out).unwrap(), "foobar\n");
}
