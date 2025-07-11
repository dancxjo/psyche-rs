//! Core types for the `psyche-rs` crate.
//!
//! This crate currently exposes [`Sensation`], [`Impression`], [`Sensor`] and
//! [`Wit`] building blocks for constructing artificial agents.

mod abort_guard;
mod cluster_analyzer;
mod combobulator;
mod conversation;
mod genius;
mod genius_queue;
mod genius_runner;
mod impression;
mod llm;
mod llm_client;
mod llm_parser;
mod memory_sensor;
mod memory_store;
mod motor;
mod motor_executor;
mod neighbor;
mod neo_qdrant_store;
mod ollama_llm;
mod plain_describe;
mod psyche;
mod psyche_event;
mod psyche_supervisor;
mod quick;
mod recent_actions;
mod retry;
mod sensation;
mod sensation_channel_sensor;
mod sensor;
mod sensor_util;
mod shutdown;
mod stream_util;
mod template;
#[cfg(test)]
pub mod test_helpers;
pub mod text_util;
mod thread_local;
mod timeline;
mod voice;
mod will;
mod wit;

pub use crate::llm::types::{Token, TokenStream};
pub use crate::llm_client::{LLMClient, spawn_llm_task};
pub use crate::ollama_llm::OllamaLLM;
pub use abort_guard::AbortGuard;
pub use cluster_analyzer::ClusterAnalyzer;
pub use combobulator::Combobulator;
pub use conversation::Conversation;
pub use genius::Genius;
pub use genius_queue::{GeniusSender, bounded_channel};
pub use genius_runner::launch_genius;
pub use impression::Impression;
pub use memory_sensor::MemorySensor;
pub use memory_store::{InMemoryStore, MemoryStore, StoredImpression, StoredSensation};
pub use motor::{
    Action, ActionResult, Completion, Intention, Interruption, Motor, MotorError,
    SensorDirectingMotor,
};
pub use motor_executor::MotorExecutor;
pub use neighbor::merge_neighbors;
pub use neo_qdrant_store::NeoQdrantMemoryStore;
pub use plain_describe::PlainDescribe;
pub use psyche::Psyche;
pub use psyche_supervisor::PsycheSupervisor;
pub use quick::{InstantInput, InstantOutput, QuickGenius};
pub use recent_actions::RecentActionsLog;
pub use retry::{RetryLLM, RetryMemoryStore, RetryPolicy};
pub use sensation::Sensation;
pub use sensation_channel_sensor::SensationSensor;
pub use sensor::Sensor;
pub use sensor_util::ImpressionStreamSensor;
pub use shutdown::shutdown_signal;
pub use template::render_template;
pub use thread_local::ThreadLocalContext;
pub use timeline::build_timeline;
pub use voice::Voice;
pub use will::{MotorDescription, Will, safe_prefix};
pub use wit::Wit;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Local;
    use futures::stream::BoxStream;
    use futures::{StreamExt, stream};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn impression_from_sensor() {
        struct TestSensor;

        impl Sensor<String> for TestSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = Sensation {
                    kind: "test".into(),
                    when: Local::now(),
                    what: "ping".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        use async_trait::async_trait;
        #[derive(Clone)]
        struct StaticLLM;

        #[async_trait]
        impl LLMClient for StaticLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use crate::llm::types::Token;
                Ok(Box::pin(stream::once(async {
                    Token {
                        text: "ping event".into(),
                    }
                })))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
            }
        }

        let llm = Arc::new(StaticLLM);
        let mut witness = Wit::new(llm).prompt("{template}").delay_ms(10);
        let s = TestSensor;
        let mut stream = witness.observe(vec![s]).await;
        if let Some(impressions) = stream.next().await {
            assert_eq!(impressions[0].how, "ping event");
        } else {
            panic!("no impression emitted");
        }
    }

    #[tokio::test]
    async fn round_trip_to_motor() {
        struct TestSensor;

        impl Sensor<String> for TestSensor {
            fn stream(&mut self) -> BoxStream<'static, Vec<Sensation<String>>> {
                let s = Sensation {
                    kind: "utterance.text".into(),
                    when: Local::now(),
                    what: "ping".into(),
                    source: None,
                };
                stream::once(async move { vec![s] }).boxed()
            }
        }

        use async_trait::async_trait;
        #[derive(Clone)]
        struct StaticLLM;

        #[async_trait]
        impl LLMClient for StaticLLM {
            async fn chat_stream(
                &self,
                _msgs: &[ollama_rs::generation::chat::ChatMessage],
            ) -> Result<TokenStream, Box<dyn std::error::Error + Send + Sync>> {
                use crate::llm::types::Token;
                Ok(Box::pin(stream::once(async {
                    Token {
                        text: "ping event".into(),
                    }
                })))
            }

            async fn embed(
                &self,
                _text: &str,
            ) -> Result<Vec<f32>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(vec![0.0])
            }
        }

        let llm = Arc::new(StaticLLM);

        struct RecordingMotor {
            log: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl Motor for RecordingMotor {
            fn description(&self) -> &'static str {
                "records actions into a vector"
            }
            fn name(&self) -> &'static str {
                "speak"
            }
            async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
                let mut action = intention.action;
                let collected = action.collect_text().await;
                self.log.lock().unwrap().push(collected);
                Ok(ActionResult {
                    sensations: Vec::new(),
                    completed: true,
                    completion: None,
                    interruption: None,
                })
            }
        }

        let mut witness = Wit::new(llm).prompt("{template}").delay_ms(10);
        let sensor = TestSensor;

        let log = Arc::new(Mutex::new(Vec::new()));
        let motor = RecordingMotor { log: log.clone() };

        let mut impressions = witness.observe(vec![sensor]).await;

        if let Some(batch) = impressions.next().await {
            for impression in batch {
                let text = impression.how.clone();
                let body = stream::once(async move { text }).boxed();
                let action = Action::new("speak", Value::Null, body);
                let intention = Intention::to(action).assign("speak");
                futures::executor::block_on(async {
                    motor.perform(intention).await.unwrap();
                });
            }
        }

        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].as_str(), "ping event");
    }
}
