use std::sync::Arc;

use llm::chat::ChatProvider;
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, info, trace};

use crate::{
    conversation::Conversation,
    countenance::Countenance,
    llm::LLMExt,
    memory::{Memory, MemoryStore, Sensation},
    motor::DummyMotor,
    mouth::Mouth,
    narrator::Narrator,
    store::NoopRetriever,
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
    /// External reflection of Pete's mood.
    pub countenance: Arc<dyn Countenance>,

    /// Shared memory store used by all components.
    pub store: Arc<dyn MemoryStore>,
    /// Language model used for generating urges.
    pub llm: Arc<dyn ChatProvider>,
    /// Incoming sensations to process.
    pub input_rx: mpsc::Receiver<Sensation>,
    /// Notifies the runtime once processing is complete.
    pub stop_tx: oneshot::Sender<()>,
}

impl Psyche {
    /// Create a new [`Psyche`] with freshly constructed cognitive wits.
    pub fn new(
        store: Arc<dyn MemoryStore>,
        llm: Arc<dyn ChatProvider>,
        mouth: Arc<dyn Mouth>,
        countenance: Arc<dyn Countenance>,
        input_rx: mpsc::Receiver<Sensation>,
        stop_tx: oneshot::Sender<()>,
        model: String,
        system_prompt: String,
        max_tokens: usize,
    ) -> Self {
        let quick = Quick::new(store.clone(), llm.clone());
        let will = Will::new(store.clone(), Arc::new(DummyMotor));
        let fond = FondDuCoeur::new(store.clone(), llm.clone());

        let narrator = Narrator {
            store: store.clone(),
            llm: llm.clone(),
            retriever: Arc::new(NoopRetriever),
        };

        let mut voice = Voice::new(narrator.clone(), mouth, store.clone());
        voice.conversation = Conversation::new(system_prompt, max_tokens);
        voice.model = model;

        Self {
            quick: Arc::new(Mutex::new(quick)),
            will: Arc::new(Mutex::new(will)),
            fond: Arc::new(Mutex::new(fond)),
            voice: Arc::new(Mutex::new(voice)),
            narrator: Arc::new(Mutex::new(narrator)),
            countenance,
            store,
            llm,
            input_rx,
            stop_tx,
        }
    }

    /// Run the processing loop until the input channel closes.
    pub async fn tick(mut self) {
        while let Some(sensation) = self.input_rx.recv().await {
            info!("📥 Received sensation: {}", sensation.kind);
            let quick = self.quick.clone();
            let will = self.will.clone();
            let fond = self.fond.clone();
            let voice = self.voice.clone();
            let store = self.store.clone();
            let llm = self.llm.clone();

            // Perception first
            let instant = {
                let mut q = quick.lock().await;
                trace!("↪ calling Quick.observe(...)");
                q.observe(sensation).await;
                trace!("↪ calling Quick.distill(...)");
                q.distill().await
            };

            if let Some(instant) = instant {
                info!("🧠 Pete thought: {}", instant.how);
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
