use async_trait::async_trait;
use battery::{Manager, State};
use chrono::Local;
use psyche_rs::{ActionResult, Completion, Intention, Motor, MotorError, Sensation};

/// Format a battery status message.
///
/// # Examples
/// ```
/// use daringsby::battery_motor::battery_message;
/// assert_eq!(battery_message(5), "My host battery is at 5% charge.");
/// ```
pub fn battery_message(pct: u8) -> String {
    format!("My host battery is at {}% charge.", pct)
}

/// Format a battery status message that includes whether the host is
/// charging, discharging or full.
///
/// # Examples
/// ```
/// use battery::State;
/// use daringsby::battery_motor::battery_status_message;
/// assert_eq!(battery_status_message(5, State::Charging),
///            "My host battery is charging at 5% charge.");
/// ```
pub fn battery_status_message(pct: u8, state: State) -> String {
    let status = match state {
        State::Charging => "charging",
        State::Discharging => "discharging",
        State::Full => "full",
        State::Empty => "empty",
        _ => "unknown",
    };
    format!("My host battery is {} at {}% charge.", status, pct)
}

/// Read the current system battery percentage.
pub fn system_battery_percentage() -> Result<u8, MotorError> {
    let manager = Manager::new().map_err(|e| MotorError::Failed(e.to_string()))?;
    let mut bats = manager
        .batteries()
        .map_err(|e| MotorError::Failed(e.to_string()))?;
    match bats.next() {
        Some(Ok(b)) => Ok((b.state_of_charge().value * 100.0).round() as u8),
        Some(Err(e)) => Err(MotorError::Failed(e.to_string())),
        None => Err(MotorError::Failed("no battery found".into())),
    }
}

/// Read the current system battery percentage and charging state.
pub fn system_battery_info() -> Result<(u8, State), MotorError> {
    let manager = Manager::new().map_err(|e| MotorError::Failed(e.to_string()))?;
    let mut bats = manager
        .batteries()
        .map_err(|e| MotorError::Failed(e.to_string()))?;
    match bats.next() {
        Some(Ok(b)) => Ok(((b.state_of_charge().value * 100.0).round() as u8, b.state())),
        Some(Err(e)) => Err(MotorError::Failed(e.to_string())),
        None => Err(MotorError::Failed("no battery found".into())),
    }
}

/// Motor that reports the host battery status on demand.
#[derive(Default)]
pub struct BatteryMotor;

#[async_trait]
impl Motor for BatteryMotor {
    fn description(&self) -> &'static str {
        "Check the host machine battery level.\n\
Parameters: none.\n\
Example:\n\
<battery></battery>\n\
Explanation:\n\
The Will reads the system battery level and emits a `battery.status` sensation."
    }

    fn name(&self) -> &'static str {
        "battery"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "battery" {
            return Err(MotorError::Unrecognized);
        }
        let pct = system_battery_percentage()?;
        let msg = battery_message(pct);
        let completion = Completion::of_action(intention.action);
        Ok(ActionResult {
            sensations: vec![Sensation {
                kind: "battery.status".into(),
                when: Local::now(),
                what: serde_json::Value::String(msg),
                source: None,
            }],
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn description_contains_example() {
        let m = BatteryMotor::default();
        assert!(m.description().contains("<battery>"));
    }

    #[test]
    fn message_function() {
        assert_eq!(battery_message(1), "My host battery is at 1% charge.");
        assert_eq!(
            battery_status_message(1, State::Charging),
            "My host battery is charging at 1% charge."
        );
    }
}
