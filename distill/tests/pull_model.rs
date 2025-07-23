use distill::{run, Config};
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Method, Request, Response, Server,
};
use ollama_rs::Ollama;
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn pulls_missing_model() {
    let hits = Arc::new(Mutex::new(0u32));
    let hits_clone = hits.clone();

    let make_service = make_service_fn(move |_| {
        let hits = hits_clone.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
                let hits = hits.clone();
                async move {
                    if req.method() == Method::POST && req.uri().path() == "/api/chat" {
                        let mut h = hits.lock().unwrap();
                        *h += 1;
                        if *h == 1 {
                            return Ok::<_, hyper::Error>(Response::builder()
                                .status(404)
                                .header("Content-Type", "application/json")
                                .body(Body::from("{\"error\":\"model \\\"phi4\\\" not found, try pulling it first\"}"))
                                .unwrap());
                        }
                        let body = "{\"model\":\"phi4\",\"created_at\":\"0\",\"message\":{\"role\":\"assistant\",\"content\":\"ok\"},\"done\":true}\n";
                        return Ok(Response::builder()
                            .header("Content-Type", "application/json")
                            .body(Body::from(body))
                            .unwrap());
                    }
                    if req.method() == Method::POST && req.uri().path() == "/api/pull" {
                        return Ok(Response::builder()
                            .header("Content-Type", "application/json")
                            .body(Body::from("{\"status\":\"success\"}\n"))
                            .unwrap());
                    }
                    Ok(Response::new(Body::empty()))
                }
            }))
        }
    });

    let server = Server::bind(&([127, 0, 0, 1], 0).into()).serve(make_service);
    let addr = server.local_addr();
    let handle = tokio::spawn(server);

    let cfg = Config {
        continuous: false,
        lines: 1,
        prompt: "Summarize: {{current}}".into(),
        model: "phi4".into(),
        terminal: "\n".into(),
    };
    let ollama = Ollama::try_new(format!("http://{}", addr)).unwrap();
    let input = BufReader::new("hello".as_bytes());
    let (mut w, mut r) = io::duplex(64);

    run(cfg, ollama, input, &mut w).await.unwrap();
    w.shutdown().await.unwrap();

    let mut out = String::new();
    r.read_to_string(&mut out).await.unwrap();
    assert_eq!(out, "ok\n\n");

    assert_eq!(*hits.lock().unwrap(), 2); // two chat calls
    handle.abort();
}
