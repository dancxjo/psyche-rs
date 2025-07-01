use crate::{HeardSelfSensor, HeardUserSensor, Heartbeat, SpeechStream};
use psyche_rs::Sensor;
use std::sync::Arc;

/// Build the basic sensors used by Daringsby.
pub fn build_sensors(stream: Arc<SpeechStream>) -> Vec<Box<dyn Sensor<String> + Send>> {
    vec![
        Box::new(Heartbeat) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardSelfSensor::new(stream.subscribe_heard())) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardUserSensor::new(stream.subscribe_user())) as Box<dyn Sensor<String> + Send>,
    ]
}
