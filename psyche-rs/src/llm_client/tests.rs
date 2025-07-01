use crate::{FairLLM, LLMClient, RoundRobinLLM, LLMTokenStream, LLMPool};
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
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
        self.log.lock().unwrap().push(self.id);
        Ok(Box::pin(stream::empty()))
    }
}

#[derive(Clone)]
struct FailingLLM;

#[async_trait]
impl LLMClient for FailingLLM {
    async fn chat_stream(
        &self,
        _msgs: &[ChatMessage],
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
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
    ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
        let d = self.delay;
        let s = stream::once(async move {
            tokio::time::sleep(std::time::Duration::from_millis(d)).await;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>("done".into())
        });
        Ok(Box::pin(s))
    }
}

#[tokio::test]
async fn round_robin_distribution() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let c1 = Arc::new(RecordLLM {
        id: 1,
        log: log.clone(),
    });
    let c2 = Arc::new(RecordLLM {
        id: 2,
        log: log.clone(),
    });
    let pool = RoundRobinLLM::new(vec![c1 as Arc<dyn LLMClient>, c2]);
    pool.chat_stream(&[]).await.unwrap().next().await;
    pool.chat_stream(&[]).await.unwrap().next().await;
    pool.chat_stream(&[]).await.unwrap().next().await;
    let l = log.lock().unwrap();
    assert_eq!(l.as_slice(), &[1, 2, 1]);
}

#[tokio::test]
async fn failover_to_next_client() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let c1 = Arc::new(FailingLLM);
    let c2 = Arc::new(RecordLLM {
        id: 2,
        log: log.clone(),
    });
    let pool = RoundRobinLLM::new(vec![c1, c2]);
    pool.chat_stream(&[]).await.unwrap().next().await;
    let l = log.lock().unwrap();
    assert_eq!(l.as_slice(), &[2]);
}

#[tokio::test]
async fn all_clients_fail() {
    let pool = RoundRobinLLM::new(vec![
        Arc::new(FailingLLM) as Arc<dyn LLMClient>,
        Arc::new(FailingLLM) as Arc<dyn LLMClient>,
    ]);
    let err = pool.chat_stream(&[]).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn concurrent_requests_use_all_clients() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let c1 = Arc::new(RecordLLM {
        id: 1,
        log: log.clone(),
    });
    let c2 = Arc::new(RecordLLM {
        id: 2,
        log: log.clone(),
    });
    let pool = RoundRobinLLM::new(vec![c1, c2]);
    let f1 = async { pool.chat_stream(&[]).await.unwrap().next().await };
    let f2 = async { pool.chat_stream(&[]).await.unwrap().next().await };
    futures::join!(f1, f2);
    let l = log.lock().unwrap();
    assert_eq!(l.len(), 2);
    assert!(l.contains(&1));
    assert!(l.contains(&2));
}

#[tokio::test]
async fn fair_llm_processes_in_request_order() {
    let llm = Arc::new(FairLLM::new(DelayLLM { delay: 50 }, 1));
    let llm2 = llm.clone();
    let start = std::time::Instant::now();
    let f1 = tokio::spawn(async move {
        let mut s = llm.chat_stream(&[]).await.unwrap();
        s.next().await.unwrap().unwrap();
    });
    let f2 = tokio::spawn(async move {
        let mut s = llm2.chat_stream(&[]).await.unwrap();
        s.next().await.unwrap().unwrap();
    });
    let _ = futures::join!(f1, f2);
    assert!(start.elapsed() >= std::time::Duration::from_millis(100));
}
#[tokio::test]
async fn llm_pool_uses_round_robin_urls() {
    let log = Arc::new(Mutex::new(Vec::new()));
    #[derive(Clone)]
    struct RecUrl {
        id: usize,
        log: Arc<Mutex<Vec<usize>>>,
    }
    #[async_trait::async_trait]
    impl LLMClient for RecUrl {
        async fn chat_stream(
            &self,
            _msgs: &[ChatMessage],
        ) -> Result<LLMTokenStream, Box<dyn std::error::Error + Send + Sync>> {
            self.log.lock().unwrap().push(self.id);
            Ok(Box::pin(stream::empty()))
        }
    }
    let c1 = Arc::new(RecUrl { id: 1, log: log.clone() });
    let c2 = Arc::new(RecUrl { id: 2, log: log.clone() });
    let pool = RoundRobinLLM::new(vec![c1.clone(), c2.clone()]);
    let llm_pool = LLMPool::from_round_robin(pool);
    llm_pool.chat_stream(&[]).await.unwrap().next().await;
    llm_pool.chat_stream(&[]).await.unwrap().next().await;
    let l = log.lock().unwrap();
    assert_eq!(l.as_slice(), &[1, 2]);
}
