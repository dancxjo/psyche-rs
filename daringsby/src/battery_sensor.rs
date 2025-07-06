use async_stream::stream;
use psyche_rs::{Sensation, Sensor};
use rand::Rng;
use tracing::debug;

use crate::battery_motor::{battery_status_message, system_battery_info};

fn interval_secs() -> u64 {
    if std::env::var("FAST_TEST").is_ok() {
        0
    } else {
        60
    }
}

/// Sensor emitting the host battery level roughly every minute.
#[derive(Default)]
pub struct BatterySensor;

impl Sensor<String> for BatterySensor {
    fn stream(&mut self) -> futures::stream::BoxStream<'static, Vec<Sensation<String>>> {
        let stream = stream! {
            loop {
                let jitter = {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(0..60)
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs() + jitter)).await;
                match system_battery_info() {
                    Ok((pct, state)) => {
                        let msg = battery_status_message(pct, state);
                        debug!(?msg, "battery sensed");
                        yield vec![Sensation {
                            kind: "battery.status".into(),
                            when: chrono::Local::now(),
                            what: msg,
                            source: None,
                        }];
                    }
                    Err(e) => {
                        tracing::warn!(error=?e, "battery sensor failed");
                    }
                }
            }
        };
        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_message() {
        use battery::State;
        assert_eq!(
            battery_status_message(42, State::Discharging),
            "My host battery is discharging at 42% charge."
        );
    }
}
