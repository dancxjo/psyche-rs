use std::sync::Arc;

use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, info};

use crate::{
    llm::LLMClient,
    memory::{Memory, MemoryStore, Sensation},
    narrator::Narrator,
    voice::Voice,
    wit::Wit,
    wits::{fond::FondDuCoeur, quick::Quick, will::Will},
};

/// Top level coordinator driving Pete's cognitive cycle.
///
/// [`Psyche`] wires together the individual cognitive wits and runs a simple
/// tick loop consuming [`Sensation`]s. Each loop observes the new sensation,
/// distills higher level impressions and intentions and finally produces output
/// via the [`Voice`].
pub struct Psyche {
    /// Short term perception summariser.
    pub quick: Arc<Mutex<Quick>>,
    /// Converts urges into executable intentions.
    pub will: Arc<Mutex<Will>>,
    /// Evaluates emotional reactions to events.
    pub fond: Arc<Mutex<FondDuCoeur>>,
    /// Pete's speaking system.
    pub voice: Arc<Mutex<Voice>>,
    /// Narrative summariser used by the voice.
    pub narrator: Arc<Mutex<Narrator>>,

    /// Shared memory store used by all components.
    pub store: Arc<dyn MemoryStore>,
    /// Language model used for generating urges.
    pub llm: Arc<dyn LLMClient>,
    /// Incoming sensations to process.
    pub input_rx: mpsc::Receiver<Sensation>,
    /// Notifies the runtime once processing is complete.
    pub stop_tx: oneshot::Sender<()>,
}

impl Psyche {
    /// Create a new [`Psyche`] bundling the given wits together.
    pub fn new(
        quick: Quick,
        will: Will,
        fond: FondDuCoeur,
        voice: Voice,
        narrator: Narrator,
        store: Arc<dyn MemoryStore>,
        llm: Arc<dyn LLMClient>,
        input_rx: mpsc::Receiver<Sensation>,
        stop_tx: oneshot::Sender<()>,
    ) -> Self {
        Self {
            quick: Arc::new(Mutex::new(quick)),
            will: Arc::new(Mutex::new(will)),
            fond: Arc::new(Mutex::new(fond)),
            voice: Arc::new(Mutex::new(voice)),
            narrator: Arc::new(Mutex::new(narrator)),
            store,
            llm,
            input_rx,
            stop_tx,
        }
    }

    /// Run the processing loop until the input channel closes.
    pub async fn tick(mut self) {
        while let Some(sensation) = self.input_rx.recv().await {
            info!("Received sensation: {:?}", sensation.kind);
            let quick = self.quick.clone();
            let will = self.will.clone();
            let fond = self.fond.clone();
            let voice = self.voice.clone();
            let store = self.store.clone();
            let llm = self.llm.clone();

            // Perception first
            let instant = {
                let mut q = quick.lock().await;
                q.observe(sensation).await;
                q.distill().await
            };

            if let Some(instant) = instant {
                info!("Distilling new Instant...");
                store.save(&Memory::Impression(instant.clone())).await.ok();

                // Suggest urges using the shared LLM then feed them into Will.
                let urges = llm.suggest_urges(&instant).await.unwrap_or_default();
                for urge in urges {
                    debug!("Observed urge: {:?}", urge.motor_name);
                    will.lock().await.observe(urge).await;
                }

                let will_task = async {
                    let mut w = will.lock().await;
                    w.distill().await
                };
                let fond_task = async {
                    fond.lock().await.distill().await;
                };
                let (intent, _) = tokio::join!(will_task, fond_task);

                if let Some(intent) = intent {
                    store.save(&Memory::Intention(intent.clone())).await.ok();
                    voice.lock().await.take_turn().await.ok();
                }
            }
        }
        let _ = self.stop_tx.send(());
    }
}
