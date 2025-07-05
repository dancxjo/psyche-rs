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

fn build_client(base_url: &str, model: &str, embed_model: &str) -> Arc<dyn LLMClient> {
    let http = Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("ollama http client");
    Arc::new(OllamaLLM::with_embedding_model(
        build_ollama(&http, base_url),
        model.to_string(),
        embed_model.to_string(),
    )) as Arc<dyn LLMClient>
}

/// Build an Ollama client dedicated to the voice loop.
pub fn build_voice_llm(args: &Args) -> Arc<dyn LLMClient> {
    let http = Client::builder()
        .pool_max_idle_per_host(10)
        .build()
        .expect("ollama http client");
    Arc::new(OllamaLLM::new(
        build_ollama(&http, &args.voice_url),
        args.voice_model.clone(),
    )) as Arc<dyn LLMClient>
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
    let quick_llm = build_client(&args.quick_url, &args.quick_model, &args.embedding_model);
    let combob_llm = build_client(&args.combob_url, &args.combob_model, &args.embedding_model);
    let will_llm = build_client(&args.will_url, &args.will_model, &args.embedding_model);
    let memory_llm = build_client(&args.memory_url, &args.memory_model, &args.embedding_model);

    (quick_llm, combob_llm, will_llm, memory_llm)
}
