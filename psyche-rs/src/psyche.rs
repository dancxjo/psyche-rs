use std::sync::Arc;

use futures_util::StreamExt;
use llm::chat::{ChatMessage, ChatProvider};
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::task::spawn_local;
use tracing::info;

use crate::{
    ear::Ear,
    llm::LLMExt,
    memory::{Impression, Intention, MemoryStore, Sensation, Urge},
    motor::{Motor, MotorEvent},
    mouth::Mouth,
    narrator::Narrator,
    store::NoopRetriever,
    voice::Voice,
    wit::{Wit, WitHandle},
    wits::{combobulator::Combobulator, quick::Quick, will::Will},
};

/// Main coordinator wiring Pete's cognitive wits into a reactive graph.
///
/// [`Psyche`] owns running instances of [`Quick`], [`Combobulator`] and
/// [`Will`]. Inputs flow from [`Sensation`] into `Quick` then on to
/// `Combobulator` and finally `Will` where actionable intentions are issued.
pub struct Psyche {
    /// Perception wit summarising raw sensations.
    pub quick: WitHandle<Sensation, Impression>,
    /// Aggregates instants into higher level situations.
    pub combobulator: WitHandle<Impression, Impression>,
    /// Decides what to do given a situation.
    pub will: WitHandle<Urge, Intention>,
    /// Pete's voice used to respond to intentions.
    pub voice: Arc<Mutex<Voice>>,
    pub ear: Ear,
    motor_tx: mpsc::Sender<MotorEvent>,
    llm: Arc<dyn ChatProvider>,
}

impl Psyche {
    /// Construct a new [`Psyche`] with all wits running on the current
    /// [`tokio::task::LocalSet`].
    pub fn new(
        store: Arc<dyn MemoryStore>,
        llm: Arc<dyn ChatProvider>,
        mouth: Arc<dyn Mouth>,
        motor: Arc<dyn Motor>,
    ) -> Self {
        // Quick wiring
        let quick = Quick::new(store.clone(), llm.clone());
        let (q_tx, q_rx) = mpsc::channel(32);
        let (instant_tx, _) = broadcast::channel(32);
        spawn_local(quick.run(q_rx, instant_tx.clone()));

        // Combobulator wiring
        let combobulator = Combobulator::new("situation", store.clone(), llm.clone());
        let (c_tx, c_rx) = mpsc::channel(32);
        let (situation_tx, _) = broadcast::channel(32);
        spawn_local(combobulator.run(c_rx, situation_tx.clone()));

        // Motor wiring
        let (motor_tx, motor_rx) = mpsc::channel(32);
        let motor_clone = motor.clone();
        spawn_local(async move {
            let _ = motor_clone.handle(motor_rx).await;
        });

        // Will wiring
        let will = Will::new(store.clone());
        let (w_tx, w_rx) = mpsc::channel(32);
        let (intent_tx, _) = broadcast::channel(32);
        spawn_local(will.run(w_rx, intent_tx.clone()));

        // Forward Quick -> Combobulator
        let mut q_out = instant_tx.subscribe();
        let c_in = c_tx.clone();
        spawn_local(async move {
            while let Ok(imp) = q_out.recv().await {
                let _ = c_in.send(imp).await;
            }
        });

        // Forward Combobulator -> Will
        let mut c_out = situation_tx.subscribe();
        let w_in = w_tx.clone();
        let llm_clone = llm.clone();
        spawn_local(async move {
            while let Ok(sit) = c_out.recv().await {
                if let Ok(urges) = llm_clone.suggest_urges(&sit).await {
                    for u in urges {
                        let _ = w_in.send(u).await;
                    }
                }
            }
        });

        // Voice and Ear wiring
        let narrator = Narrator {
            store: store.clone(),
            llm: llm.clone(),
            retriever: Arc::new(NoopRetriever),
        };
        let mut voice = Voice::new(narrator, mouth, store.clone());
        voice.llm = llm.clone();
        let voice = Arc::new(Mutex::new(voice));

        let ear = Ear::new(q_tx.clone());

        // React to issued intentions by speaking a turn and streaming to the motor
        let mut intent_sub = intent_tx.subscribe();
        let voice_clone = voice.clone();
        let ear_clone = ear.clone();
        let motor_sender = motor_tx.clone();
        let llm_clone2 = llm.clone();
        spawn_local(async move {
            while let Ok(intent) = intent_sub.recv().await {
                info!("🎤 Voice reacting to intent: {}", intent.action.name);

                // motor event stream
                let tx = motor_sender.clone();
                let intent_clone = intent.clone();
                let llm_inner = llm_clone2.clone();
                spawn_local(async move {
                    let _ = tx.send(MotorEvent::Begin(intent_clone.clone())).await;
                    if let Ok(mut stream) = llm_inner
                        .chat_stream(&[ChatMessage::user()
                            .content(intent_clone.action.name.clone())
                            .build()])
                        .await
                    {
                        while let Some(chunk) = stream.next().await {
                            if let Ok(text) = chunk {
                                let _ = tx.send(MotorEvent::Chunk(text)).await;
                            }
                        }
                    }
                    let _ = tx.send(MotorEvent::End).await;
                });

                // voice reaction
                let mut v = voice_clone.lock().await;
                if let Ok(spoken) = v.take_turn().await {
                    info!("🔊 Spoken: {}", spoken);
                    ear_clone.hear_self(&spoken).await;
                }
            }
        });

        Self {
            quick: WitHandle {
                sender: q_tx,
                receiver: instant_tx.subscribe(),
            },
            combobulator: WitHandle {
                sender: c_tx,
                receiver: situation_tx.subscribe(),
            },
            will: WitHandle {
                sender: w_tx,
                receiver: intent_tx.subscribe(),
            },
            voice,
            ear,
            motor_tx,
            llm,
        }
    }

    /// Send a [`Sensation`] into the cognitive pipeline.
    pub async fn send_sensation(
        &self,
        s: Sensation,
    ) -> Result<(), mpsc::error::SendError<Sensation>> {
        info!("📥 Received sensation: {}", s.kind);
        self.quick.sender.send(s).await
    }
}
