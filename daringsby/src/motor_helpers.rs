use crate::{
    CanvasMotor, CanvasStream, LogMemoryMotor, LoggingMotor, Mouth, RecallMotor, SourceReadMotor,
    SourceSearchMotor, SourceTreeMotor, SvgMotor, VisionMotor, VisionSensor,
};
use psyche_rs::{InMemoryStore, LLMClient, Motor, Sensation};
use std::{collections::HashMap, sync::Arc};

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
    canvas: Arc<CanvasStream>,
    store: Arc<InMemoryStore>,
) -> (Vec<Arc<dyn Motor>>, Arc<HashMap<String, Arc<dyn Motor>>>) {
    use tokio::sync::mpsc::unbounded_channel;

    let (look_tx, _look_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (canvas_tx, _canvas_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (svg_tx, svg_rx) = unbounded_channel::<String>();
    let (_thought_tx, _thought_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (recall_tx, _recall_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (log_mem_tx, _log_mem_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (read_tx, _read_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (search_tx, _search_rx) = unbounded_channel::<Vec<Sensation<String>>>();
    let (tree_tx, _tree_rx) = unbounded_channel::<Vec<Sensation<String>>>();

    let logging_motor = Arc::new(LoggingMotor);
    let vision_motor = Arc::new(VisionMotor::new(vision, llms.quick.clone(), look_tx));
    let canvas_motor = Arc::new(CanvasMotor::new(canvas, llms.quick.clone(), canvas_tx));
    let svg_motor = Arc::new(SvgMotor::new(svg_tx));
    let recall_motor = Arc::new(RecallMotor::new(
        store.clone(),
        llms.memory.clone(),
        recall_tx,
        5,
    ));
    let log_memory_motor = Arc::new(LogMemoryMotor::new(log_mem_tx));
    let source_read_motor = Arc::new(SourceReadMotor::new(read_tx));
    let source_search_motor = Arc::new(SourceSearchMotor::new(search_tx));
    let source_tree_motor = Arc::new(SourceTreeMotor::new(tree_tx));

    let motors: Vec<Arc<dyn Motor>> = vec![
        logging_motor.clone(),
        vision_motor.clone(),
        mouth.clone(),
        canvas_motor.clone(),
        svg_motor.clone(),
        recall_motor.clone(),
        log_memory_motor.clone(),
        source_read_motor.clone(),
        source_search_motor.clone(),
        source_tree_motor.clone(),
    ];

    let mut map = HashMap::new();
    map.insert("log".into(), logging_motor as Arc<dyn Motor>);
    map.insert("look".into(), vision_motor as Arc<dyn Motor>);
    map.insert("say".into(), mouth as Arc<dyn Motor>);
    map.insert("canvas".into(), canvas_motor as Arc<dyn Motor>);
    map.insert("draw".into(), svg_motor as Arc<dyn Motor>);
    map.insert("recall".into(), recall_motor as Arc<dyn Motor>);
    map.insert("read_log_memory".into(), log_memory_motor as Arc<dyn Motor>);
    map.insert("read_source".into(), source_read_motor as Arc<dyn Motor>);
    map.insert(
        "search_source".into(),
        source_search_motor as Arc<dyn Motor>,
    );
    map.insert("source_tree".into(), source_tree_motor as Arc<dyn Motor>);

    let _ = svg_rx; // keep channel alive

    (motors, Arc::new(map))
}
