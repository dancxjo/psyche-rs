use llm::chat::{ChatMessage, ChatProvider, ChatResponse};
use psyche_rs::memory::Sensation;
use psyche_rs::{DummyMouth, DummyStore, Psyche};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::LocalSet;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt::init();

    let (tx, mut rx) = mpsc::channel(32);

    struct DummyLLM;
    #[async_trait::async_trait]
    impl ChatProvider for DummyLLM {
        async fn chat_with_tools(
            &self,
            _m: &[ChatMessage],
            _t: Option<&[llm::chat::Tool]>,
        ) -> Result<Box<dyn ChatResponse>, llm::error::LLMError> {
            Ok(Box::new(SimpleResp("".into())))
        }
        async fn chat_stream(
            &self,
            msgs: &[ChatMessage],
        ) -> Result<
            Pin<Box<dyn futures_util::Stream<Item = Result<String, llm::error::LLMError>> + Send>>,
            llm::error::LLMError,
        > {
            let reply = msgs.last().unwrap().content.clone();
            Ok(Box::pin(futures_util::stream::once(
                async move { Ok(reply) },
            )))
        }
    }

    #[derive(Debug)]
    struct SimpleResp(String);
    impl ChatResponse for SimpleResp {
        fn text(&self) -> Option<String> {
            Some(self.0.clone())
        }
        fn tool_calls(&self) -> Option<Vec<llm::ToolCall>> {
            None
        }
    }
    impl std::fmt::Display for SimpleResp {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    let psyche = Psyche::new(
        Arc::new(DummyStore::new()),
        Arc::new(DummyLLM),
        Arc::new(DummyMouth),
    );
    let local = LocalSet::new();
    local
        .run_until(async move {
            tokio::task::spawn_local(async move {
                while let Some(s) = rx.recv().await {
                    let _ = psyche.send_sensation(s).await;
                }
            });
            for i in 0..3 {
                let s = Sensation::new_text(format!("This is event {}", i), "test");
                tx.send(s).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        })
        .await;
}
