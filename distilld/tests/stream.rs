use distilld::{run, Config};
use httpmock::prelude::*;
use httpmock::Method;
use ollama_rs::Ollama;
use tokio::io::BufReader;

#[tokio::test]
async fn run_streams_output() {
    let server = MockServer::start_async().await;
    let body = concat!(
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"foo\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"bar\",\"done\":true}\n"
    );
    server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/generate");
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
        terminal: "\n".into(),
        history_depth: 1,
        beat: 0,
        trim_newlines: true,
    };
    let input = BufReader::new("hi".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    assert_eq!(std::str::from_utf8(&out).unwrap(), "foobar\n");
}

#[tokio::test]
async fn run_trims_newlines() {
    let server = MockServer::start_async().await;
    let body = concat!(
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"foo\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"bar\",\"done\":true}\n"
    );
    server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/generate");
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
        terminal: "\n".into(),
        history_depth: 1,
        beat: 0,
        trim_newlines: true,
    };
    let input = BufReader::new("hi".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    assert_eq!(std::str::from_utf8(&out).unwrap(), "foobar\n");
}

#[tokio::test]
async fn run_no_trim_includes_newlines() {
    let server = MockServer::start_async().await;
    let body = concat!(
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"foo\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"bar\",\"done\":true}\n"
    );
    server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/generate");
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
        terminal: "\n".into(),
        history_depth: 1,
        beat: 0,
        trim_newlines: false,
    };
    let input = BufReader::new("hi".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    assert_eq!(std::str::from_utf8(&out).unwrap(), "foo\nbar\n");
}

#[tokio::test]
async fn run_skips_empty_input_lines() {
    let server = MockServer::start_async().await;
    let body = "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"ok\",\"done\":true}\n";
    let mock = server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/generate");
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
        terminal: "\n".into(),
        history_depth: 1,
        beat: 0,
        trim_newlines: true,
    };
    let input = BufReader::new("foo\n\nbar\n".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    assert_eq!(std::str::from_utf8(&out).unwrap(), "ok\nok\n");
    assert_eq!(mock.hits_async().await, 2);
}

#[tokio::test]
async fn run_filters_empty_output_lines() {
    let server = MockServer::start_async().await;
    let body = concat!(
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"foo\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"bar\",\"done\":true}\n"
    );
    server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/generate");
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
        terminal: "\n".into(),
        history_depth: 1,
        beat: 0,
        trim_newlines: true,
    };
    let input = BufReader::new("hi".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    assert_eq!(std::str::from_utf8(&out).unwrap(), "foobar\n");
}

#[tokio::test]
async fn run_no_trim_preserves_blank_lines() {
    let server = MockServer::start_async().await;
    let body = concat!(
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"foo\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"bar\",\"done\":true}\n"
    );
    server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/generate");
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
        terminal: "\n".into(),
        history_depth: 1,
        beat: 0,
        trim_newlines: false,
    };
    let input = BufReader::new("hi".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    assert_eq!(std::str::from_utf8(&out).unwrap(), "foo\n\nbar\n");
}

#[tokio::test]
async fn run_skips_empty_summaries() {
    let server = MockServer::start_async().await;
    let body = concat!(
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":true}\n",
    );
    server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/generate");
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
        terminal: "\n".into(),
        history_depth: 1,
        beat: 0,
        trim_newlines: true,
    };
    let input = BufReader::new("hi".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    eprintln!("out={:?}", std::str::from_utf8(&out).unwrap());
    assert!(std::str::from_utf8(&out).unwrap().is_empty());
}
#[tokio::test]
async fn run_no_trim_skips_empty_summaries() {
    let server = MockServer::start_async().await;
    let body = concat!(
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":false}\n",
        "{\"model\":\"llama3\",\"created_at\":\"now\",\"response\":\"\\n\",\"done\":true}\n",
    );
    server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/api/generate");
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
        terminal: "\n".into(),
        history_depth: 1,
        beat: 0,
        trim_newlines: false,
    };
    let input = BufReader::new("hi".as_bytes());
    let mut out = Vec::new();
    run(cfg, ollama, input, &mut out).await.unwrap();
    eprintln!("new out={:?}", std::str::from_utf8(&out).unwrap());
    assert!(std::str::from_utf8(&out).unwrap().is_empty());
}
