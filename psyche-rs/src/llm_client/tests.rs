use crate::{spawn_llm_task, LLMClient};
use crate::llm::types::{Token, TokenStream};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use ollama_rs::generation::chat::ChatMessage;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct RecordLLM {
    id: usize,
    log: Arc<Mutex<Vec<usize>>>,
}

#[async_trait]
impl LLMClient for RecordLLM {
    async fn chat_stream(
        &self,
        _msgs: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        self.log.lock().unwrap().push(self.id);
        Ok(Box::pin(stream::empty()))
    }

    async fn embed(
        &self,
        _text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![self.id as f32])
    }
}

#[derive(Clone)]
struct FailingLLM;

#[async_trait]
impl LLMClient for FailingLLM {
    async fn chat_stream(
        &self,
        _msgs: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "fail",
        )))
    }

    async fn embed(
        &self,
        _text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "fail",
        )))
    }
}

#[derive(Clone)]
struct DelayLLM {
    delay: u64,
}

#[async_trait]
impl LLMClient for DelayLLM {
    async fn chat_stream(
        &self,
        _msgs: &[ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let d = self.delay;
        let s = stream::once(async move {
            tokio::time::sleep(std::time::Duration::from_millis(d)).await;
            Token { text: "done".into() }
        });
        Ok(Box::pin(s))
    }

    async fn embed(
        &self,
        _text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        tokio::time::sleep(std::time::Duration::from_millis(self.delay)).await;
        Ok(vec![0.0])
    }
}


#[tokio::test]
async fn spawn_llm_task_collects_tokens() {
    let llm = Arc::new(crate::test_helpers::StaticLLM::new("hello world"));
    let handle = spawn_llm_task(llm, vec![ChatMessage::user("hi".into())]).await;
    let text = handle.await.unwrap().unwrap();
    assert_eq!(text.trim(), "hello world");
}

