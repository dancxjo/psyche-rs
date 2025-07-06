use async_stream::stream;
use psyche_rs::{Sensation, Sensor};
use rand::Rng;
use tracing::debug;

use crate::battery_motor::{battery_message, system_battery_percentage};

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
                match system_battery_percentage() {
                    Ok(pct) => {
                        let msg = battery_message(pct);
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
        assert_eq!(battery_message(42), "My host battery is at 42% charge.");
    }
}
