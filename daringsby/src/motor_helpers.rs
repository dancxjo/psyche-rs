use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc::unbounded_channel;

use crate::Mouth;
use psyche_rs::{LLMClient, MemoryStore, Motor, Sensation};

#[cfg(feature = "vision")]
use crate::{VisionMotor, VisionSensor};

#[cfg(feature = "canvas-motor")]
use crate::{CanvasMotor, CanvasStream};

#[cfg(feature = "svg-motor")]
use crate::SvgMotor;

#[cfg(feature = "recall-motor")]
use crate::RecallMotor;

#[cfg(feature = "log-memory-motor")]
use crate::LogMemoryMotor;

#[cfg(feature = "source-read-motor")]
use crate::SourceReadMotor;

#[cfg(feature = "source-search-motor")]
use crate::SourceSearchMotor;

#[cfg(feature = "source-tree-motor")]
use crate::SourceTreeMotor;

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
    #[cfg(feature = "mouth")] mouth: Arc<Mouth>,
    #[cfg(feature = "vision")] vision: Arc<VisionSensor>,
    #[cfg(feature = "canvas-motor")] canvas: Arc<CanvasStream>,
    store: Arc<dyn MemoryStore + Send + Sync>,
) -> (Vec<Arc<dyn Motor>>, Arc<HashMap<String, Arc<dyn Motor>>>) {
    let mut motors: Vec<Arc<dyn Motor>> = Vec::new();
    let mut map: HashMap<String, Arc<dyn Motor>> = HashMap::new();

    // let logging_motor: Arc<dyn Motor> = Arc::new(LoggingMotor);
    // motors.push(logging_motor.clone());
    // map.insert("log".into(), logging_motor);

    #[cfg(feature = "vision")]
    {
        let (look_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let vision_motor = Arc::new(VisionMotor::new(vision, llms.quick.clone(), look_tx));
        motors.push(vision_motor.clone());
        map.insert("look".into(), vision_motor);
    }

    #[cfg(feature = "mouth")]
    {
        motors.push(mouth.clone());
        map.insert("speak".into(), mouth);
    }

    #[cfg(feature = "canvas-motor")]
    {
        let (canvas_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let canvas_motor = Arc::new(CanvasMotor::new(canvas, llms.quick.clone(), canvas_tx));
        motors.push(canvas_motor.clone());
        map.insert("canvas".into(), canvas_motor);
    }

    #[cfg(feature = "svg-motor")]
    {
        let (svg_tx, _svg_rx) = unbounded_channel::<String>();
        let svg_motor = Arc::new(SvgMotor::new(svg_tx));
        motors.push(svg_motor.clone());
        map.insert("draw".into(), svg_motor);
    }

    #[cfg(feature = "recall-motor")]
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

    #[cfg(feature = "log-memory-motor")]
    {
        let (log_mem_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let log_memory_motor = Arc::new(LogMemoryMotor::new(log_mem_tx));
        motors.push(log_memory_motor.clone());
        map.insert("read_log_memory".into(), log_memory_motor);
    }

    #[cfg(feature = "source-read-motor")]
    {
        let (read_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let source_read_motor = Arc::new(SourceReadMotor::new(read_tx));
        motors.push(source_read_motor.clone());
        map.insert("read_source".into(), source_read_motor);
    }

    #[cfg(feature = "source-search-motor")]
    {
        let (search_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let source_search_motor = Arc::new(SourceSearchMotor::new(search_tx));
        motors.push(source_search_motor.clone());
        map.insert("search_source".into(), source_search_motor);
    }

    #[cfg(feature = "source-tree-motor")]
    {
        let (tree_tx, _) = unbounded_channel::<Vec<Sensation<String>>>();
        let source_tree_motor = Arc::new(SourceTreeMotor::new(tree_tx));
        motors.push(source_tree_motor.clone());
        map.insert("source_tree".into(), source_tree_motor);
    }

    (motors, Arc::new(map))
}
