use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    Mutex,
    mpsc::{UnboundedReceiver, unbounded_channel},
};

use crate::Mouth;
use crate::memory_consolidation_motor::MemoryConsolidationMotor;
use crate::memory_consolidation_sensor::ConsolidationStatus;
use psyche_rs::{ClusterAnalyzer, LLMClient, MemoryStore, Motor, Sensation};

use crate::{VisionMotor, VisionSensor};

use crate::RecallMotor;

use crate::LogMemoryMotor;

use crate::SourceReadMotor;

use crate::SourceSearchMotor;

use crate::BatteryMotor;
use crate::SourceTreeMotor;
use crate::identify_motor::IdentifyMotor;
use reqwest::Client;
use url::Url;

/// Container for the four LLM clients used by Daringsby.
pub struct LLMClients {
    pub quick: Arc<dyn LLMClient>,
    pub combob: Arc<dyn LLMClient>,
    pub will: Arc<dyn LLMClient>,
    pub memory: Arc<dyn LLMClient>,
}

/// Build all motors used by the application and a lookup map by name.
#[allow(clippy::too_many_lines)]
#[allow(clippy::type_complexity)]
pub fn build_motors(
    llms: &LLMClients,
    mouth: Arc<Mouth>,
    vision: Arc<VisionSensor>,
    store: Arc<dyn MemoryStore + Send + Sync>,
    neo4j_url: Url,
    neo4j_user: String,
    neo4j_pass: String,
) -> (
    Vec<Arc<dyn Motor>>,
    Arc<HashMap<String, Arc<dyn Motor>>>,
    Option<Arc<Mutex<ConsolidationStatus>>>,
    Option<UnboundedReceiver<Vec<Sensation<String>>>>,
) {
    let mut motors: Vec<Arc<dyn Motor>> = Vec::new();
    let mut map: HashMap<String, Arc<dyn Motor>> = HashMap::new();
    let look_rx_opt: Option<UnboundedReceiver<Vec<Sensation<String>>>>;

    let status: Option<Arc<Mutex<ConsolidationStatus>>> =
        Some(Arc::new(Mutex::new(ConsolidationStatus::default())));

    // let logging_motor: Arc<dyn Motor> = Arc::new(LoggingMotor);
    // motors.push(logging_motor.clone());
    // map.insert("log".into(), logging_motor);

    {
        let (look_tx, look_rx) = unbounded_channel::<Vec<Sensation<String>>>();
        look_rx_opt = Some(look_rx);
        let vision_motor = Arc::new(VisionMotor::new(vision, llms.quick.clone(), look_tx));
        motors.push(vision_motor.clone());
        map.insert("look".into(), vision_motor);
    }

    {
        motors.push(mouth.clone());
        map.insert("speak".into(), mouth);
    }

    {
        let (recall_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let recall_motor = Arc::new(RecallMotor::new(
            store.clone(),
            llms.memory.clone(),
            recall_tx,
            5,
        ));
        motors.push(recall_motor.clone());
        map.insert("recall".into(), recall_motor);
    }

    {
        let (log_mem_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let log_memory_motor = Arc::new(LogMemoryMotor::new(log_mem_tx));
        motors.push(log_memory_motor.clone());
        map.insert("read_log_memory".into(), log_memory_motor);
    }

    {
        let (mc_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let analyzer = Arc::new(ClusterAnalyzer::new(store.clone(), llms.memory.clone()));
        let mc_motor = Arc::new(MemoryConsolidationMotor::new(
            analyzer,
            status.as_ref().unwrap().clone(),
            mc_tx,
        ));
        motors.push(mc_motor.clone());
        map.insert("consolidate".into(), mc_motor);
    }

    {
        let (read_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let source_read_motor = Arc::new(SourceReadMotor::new(read_tx));
        motors.push(source_read_motor.clone());
        map.insert("read_source".into(), source_read_motor);
    }

    {
        let (search_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let source_search_motor = Arc::new(SourceSearchMotor::new(search_tx));
        motors.push(source_search_motor.clone());
        map.insert("search_source".into(), source_search_motor);
    }

    {
        let (tree_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let source_tree_motor = Arc::new(SourceTreeMotor::new(tree_tx));
        motors.push(source_tree_motor.clone());
        map.insert("source_tree".into(), source_tree_motor);
    }

    {
        let (id_tx, _) = unbounded_channel::<Vec<Sensation<serde_json::Value>>>();
        let identify_motor = Arc::new(IdentifyMotor::new(
            Client::new(),
            neo4j_url.clone(),
            neo4j_user.clone(),
            neo4j_pass.clone(),
            id_tx,
        ));
        motors.push(identify_motor.clone());
        map.insert("identify".into(), identify_motor);
    }

    {
        let battery_motor = Arc::new(BatteryMotor::default());
        motors.push(battery_motor.clone());
        map.insert("battery".into(), battery_motor);
    }

    (motors, Arc::new(map), status, look_rx_opt)
}
