#![cfg(test)]

use crate::llm::types::{Token, TokenStream};
use crate::{Impression, LLMClient, Sensation, Sensor};
use async_trait::async_trait;
use futures::stream::{self, BoxStream};

/// [`LLMClient`] that returns a fixed reply split into whitespace separated tokens.
#[derive(Clone)]
pub struct StaticLLM {
    pub reply: String,
}

impl StaticLLM {
    pub fn new(reply: impl Into<String>) -> Self {
        Self {
            reply: reply.into(),
        }
    }
}

#[async_trait]
impl LLMClient for StaticLLM {
    async fn chat_stream(
        &self,
        _msgs: &[ollama_rs::generation::chat::ChatMessage],
    ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let words: Vec<Token> = self
            .reply
            .split_whitespace()
            .map(|w| Token {
                text: format!("{} ", w),
            })
            .collect();
        let s = stream::iter(words);
        Ok(Box::pin(s))
    }

    async fn embed(
        &self,
        _text: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![0.0])
    }
}

/// Simple sensor yielding a single [`Impression<String>`].
pub struct TestSensor;

impl Sensor<Impression<String>> for TestSensor {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<Impression<String>>>> {
        let imp = Impression {
            how: "hello".into(),
            what: Vec::new(),
        };
        let s = Sensation {
            kind: "impression".into(),
            when: chrono::Local::now(),
            what: imp,
            source: None,
        };
        Box::pin(stream::once(async move { vec![s] }))
    }
}

/// Sensor emitting two batches with a short delay.
pub struct TwoBatch;

impl Sensor<Impression<String>> for TwoBatch {
    fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<Impression<String>>>> {
        use async_stream::stream;
        let s = stream! {
            yield vec![Sensation {
                kind: "impression".into(),
                when: chrono::Local::now(),
                what: Impression { how: "a".into(), what: Vec::new() },
                source: None,
            }];
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            yield vec![Sensation {
                kind: "impression".into(),
                when: chrono::Local::now(),
                what: Impression { how: "b".into(), what: Vec::new() },
                source: None,
            }];
        };
        Box::pin(s)
    }
}
