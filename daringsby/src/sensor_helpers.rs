use crate::BatterySensor;
use crate::development_status::DevelopmentStatus;
use crate::memory_consolidation_sensor::{ConsolidationStatus, MemoryConsolidationSensor};
use crate::{Ear, HeardSelfSensor, HeardUserSensor, Heartbeat, SpeechStream};
use crate::{SelfDiscovery, SourceDiscovery};
use psyche_rs::Sensor;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

/// Build the sensors used by Daringsby.
///
/// When the `development-status-sensor` feature is enabled, the
/// [`DevelopmentStatus`] sensor is also returned.
///
/// # Examples
/// ```no_run
/// use daringsby::sensor_helpers::build_sensors;
/// use daringsby::SpeechStream;
/// use std::sync::Arc;
/// use tokio::sync::broadcast;
///
/// unsafe { std::env::set_var("FAST_TEST", "1") };
/// let (_a_tx, a_rx) = broadcast::channel(1);
/// let (_t_tx, t_rx) = broadcast::channel(1);
/// let (_s_tx, s_rx) = broadcast::channel(1);
/// let stream = Arc::new(SpeechStream::new(a_rx, t_rx, s_rx));
/// let sensors = build_sensors(stream, None);
/// assert!(!sensors.is_empty());
/// ```
pub fn build_sensors(
    stream: Arc<SpeechStream>,
    consolidation: Option<Arc<Mutex<ConsolidationStatus>>>,
) -> Vec<Box<dyn Sensor<String> + Send>> {
    let mut sensors: Vec<Box<dyn Sensor<String> + Send>> = vec![
        Box::new(Heartbeat) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardSelfSensor::new(stream.subscribe_heard())) as Box<dyn Sensor<String> + Send>,
        Box::new(HeardUserSensor::new(stream.subscribe_user())) as Box<dyn Sensor<String> + Send>,
        Box::new(SelfDiscovery::default()) as Box<dyn Sensor<String> + Send>,
        Box::new(SourceDiscovery::default()) as Box<dyn Sensor<String> + Send>,
    ];
    {
        sensors.push(Box::new(BatterySensor::default()) as Box<dyn Sensor<String> + Send>);
    }
    {
        debug!("development status sensor plugged in");
        sensors.push(Box::new(DevelopmentStatus) as Box<dyn Sensor<String> + Send>);
    }
    if let Some(status) = consolidation {
        sensors.push(
            Box::new(MemoryConsolidationSensor::new(status)) as Box<dyn Sensor<String> + Send>
        );
    }
    sensors
}

/// Build the [`Ear`] sensor combining heard self and user speech.
pub fn build_ear(stream: Arc<SpeechStream>) -> Ear {
    Ear::from_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio::sync::broadcast;

    #[tokio::test]
    #[ignore]
    async fn includes_development_status_sensor() {
        unsafe { std::env::set_var("FAST_TEST", "1") };
        let (_a_tx, a_rx) = broadcast::channel(1);
        let (_t_tx, t_rx) = broadcast::channel(1);
        let (_s_tx, s_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(a_rx, t_rx, s_rx));
        let sensors = build_sensors(stream, None);
        let mut found = false;
        for mut sensor in sensors {
            if let Some(batch) = sensor.stream().next().await {
                if batch.iter().any(|s| s.kind == "development_status") {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "development status sensor not included");
    }

    #[tokio::test]
    async fn discovery_sensors_report() {
        unsafe { std::env::set_var("FAST_TEST", "1") };
        let (_a_tx, a_rx) = broadcast::channel(1);
        let (_t_tx, t_rx) = broadcast::channel(1);
        let (_s_tx, s_rx) = broadcast::channel(1);
        let stream = Arc::new(SpeechStream::new(a_rx, t_rx, s_rx));
        let sensors = build_sensors(stream, None);
        let mut kinds = Vec::new();
        for mut sensor in sensors {
            if let Some(batch) = sensor.stream().next().await {
                kinds.extend(batch.iter().map(|s| s.kind.clone()));
            }
        }
        assert!(kinds.contains(&"self_discovery".to_string()));
        assert!(kinds.contains(&"self_source".to_string()));
    }
}
