use crate::args::Args;
use psyche_rs::{LLMClient, OllamaLLM};
use reqwest::Client;
use std::sync::Arc;
use url::Url;

fn build_ollama(client: &Client, base: &str) -> ollama_rs::Ollama {
    let url = Url::parse(base).expect("invalid base url");
    let host = format!("{}://{}", url.scheme(), url.host_str().expect("no host"));
    let port = url.port_or_known_default().expect("no port");
    ollama_rs::Ollama::new_with_client(host, port, client.clone())
}

/// Build all Ollama LLM clients.
pub fn build_ollama_clients(
    args: &Args,
) -> (
    Arc<dyn LLMClient>,
    Arc<dyn LLMClient>,
    Arc<dyn LLMClient>,
    Arc<dyn LLMClient>,
) {
    let quick_http = Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("quick http client");
    let combob_http = Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("combob http client");
    let will_http = Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("will http client");

    let quick_llm: Arc<dyn LLMClient> = Arc::new(OllamaLLM::new(
        build_ollama(&quick_http, &args.quick_url),
        args.quick_model.clone(),
    ));
    let combob_llm: Arc<dyn LLMClient> = Arc::new(OllamaLLM::new(
        build_ollama(&combob_http, &args.combob_url),
        args.combob_model.clone(),
    ));
    let will_llm: Arc<dyn LLMClient> = Arc::new(OllamaLLM::new(
        build_ollama(&will_http, &args.will_url),
        args.will_model.clone(),
    ));
    let memory_llm: Arc<dyn LLMClient> = Arc::new(OllamaLLM::new(
        build_ollama(&quick_http, &args.quick_url),
        args.memory_model.clone(),
    ));

    (quick_llm, combob_llm, will_llm, memory_llm)
}
