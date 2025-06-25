use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};

use crate::{
    llm::LLMClient,
    memory::{Memory, MemoryStore, Sensation},
    motor::MotorSystem,
    mouth::Mouth,
    narrator::Narrator,
    voice::Voice,
    wit::Wit,
    wits::{fond::FondDuCoeur, quick::Quick, will::Will},
};

/// Central orchestrator coordinating Pete's cognitive wits.
///
/// [`Psyche`] owns the various wits and drives their cooperation in a simple
/// tick loop.  Sensations are pushed via an internal channel which the loop
/// consumes one by one.  Each tick performs the following high level steps:
///
/// 1. Feed the [`Quick`] wit new sensations.
/// 2. Persist any distilled [`Impression`]s and resulting [`Urge`]s.
/// 3. Let [`Will`] convert urges into intentions, executing motors.
/// 4. Have the [`Voice`] speak whenever an intention is produced.
/// 5. Reflect on motor feedback via [`FondDuCoeur`] to create emotions.
///
/// The loop terminates when the input channel is closed or when the provided
/// `stop_tx` is signalled.
pub struct Psyche {
    /// Shared memory store for all components.
    pub store: Arc<dyn MemoryStore>,
    /// Short term perception summariser.
    pub quick: Mutex<Quick>,
    /// Converts urges into executable intentions.
    pub will: Mutex<Will>,
    /// Evaluates emotional reactions to events.
    pub fond: Mutex<FondDuCoeur>,
    /// Pete's speaking system.
    pub voice: Mutex<Voice>,
    /// Narrative summariser used by the voice.
    pub narrator: Mutex<Narrator>,
    /// Language model backing the wits.
    pub llm: Arc<dyn LLMClient>,
    input_rx: Mutex<mpsc::Receiver<Sensation>>, // protected for &self access
    input_tx: mpsc::Sender<Sensation>,
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl Psyche {
    /// Construct a new [`Psyche`] with all wits wired together.
    pub fn new(
        store: Arc<dyn MemoryStore>,
        llm: Arc<dyn LLMClient>,
        motor: Arc<dyn MotorSystem>,
        mouth: Arc<dyn Mouth>,
        input_tx: mpsc::Sender<Sensation>,
        input_rx: mpsc::Receiver<Sensation>,
        stop_tx: oneshot::Sender<()>,
    ) -> Self {
        let narrator = Narrator {
            store: store.clone(),
            llm: llm.clone(),
        };
        let voice = Voice::new(narrator.clone(), mouth, store.clone());
        Self {
            store: store.clone(),
            quick: Mutex::new(Quick::new(store.clone(), llm.clone())),
            will: Mutex::new(Will::new(store.clone(), motor)),
            fond: Mutex::new(FondDuCoeur::new(store.clone(), llm.clone())),
            voice: Mutex::new(voice),
            narrator: Mutex::new(narrator),
            llm,
            input_rx: Mutex::new(input_rx),
            input_tx,
            stop_tx: Mutex::new(Some(stop_tx)),
        }
    }

    /// Push a new sensation onto the processing queue.
    pub async fn push_sensation(
        &self,
        s: Sensation,
    ) -> Result<(), mpsc::error::SendError<Sensation>> {
        self.input_tx.send(s).await
    }

    /// Forward a prompt to Pete's [`Voice`].
    pub async fn ask(&self, prompt: &str) -> anyhow::Result<String> {
        let mut voice = self.voice.lock().unwrap();
        voice
            .conversation
            .hear(crate::conversation::Role::Interlocutor, prompt);
        let reply = voice.take_turn().await?;
        Ok(reply)
    }

    /// Run a single processing loop until the input channel closes.
    pub async fn tick(&self) {
        loop {
            let next = { self.input_rx.lock().unwrap().recv().await };
            let Some(sensation) = next else { break };

            // quick perception
            self.quick.lock().unwrap().observe(sensation).await;

            if let Some(instant) = self.quick.lock().unwrap().distill().await {
                let _ = self.store.save(&Memory::Impression(instant.clone())).await;

                let urges = self.llm.suggest_urges(&instant).await.unwrap_or_default();
                for urge in urges {
                    self.will.lock().unwrap().observe(urge).await;
                }

                if let Some(intent) = self.will.lock().unwrap().distill().await {
                    let _ = self.store.save(&Memory::Intention(intent.clone())).await;
                    let _ = self.voice.lock().unwrap().take_turn().await;
                }
            }

            self.fond.lock().unwrap().distill().await;
        }

        if let Some(tx) = self.stop_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}
