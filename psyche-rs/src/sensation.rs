use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// A generic Sensation type for the Pete runtime.
///
/// The payload type `T` defaults to [`serde_json::Value`].
///
/// # Examples
///
/// Creating a typed sensation:
///
/// ```
/// use chrono::Local;
/// use psyche_rs::Sensation;
///
/// let s: Sensation<String> = Sensation {
///     kind: "utterance.text".into(),
///     when: Local::now(),
///     what: "hello".into(),
///     source: Some("interlocutor".into()),
/// };
/// assert_eq!(s.what, "hello");
/// ```
/// The field `when` always contains local time, so that LLMs more naturally
/// integrate time references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sensation<T = serde_json::Value> {
    /// Category of sensation, e.g. `"utterance.text"`.
    pub kind: String,
    /// Timestamp for when the sensation occurred. Local time is stored to ease
    /// interpretation by humans and language models.
    #[serde(with = "local_time_format")]
    pub when: DateTime<Local>,
    /// Payload describing what was sensed.
    pub what: T,
    /// Optional origin identifier.
    pub source: Option<String>,
}

mod local_time_format {
    use super::*;
    use chrono::{NaiveDateTime, TimeZone};
    use serde::{Deserializer, Serializer};

    const FORMAT: &str = "%Y-%m-%d %H:%M:%S";

    pub fn serialize<S>(dt: &DateTime<Local>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ser.serialize_str(&dt.format(FORMAT).to_string())
    }

    pub fn deserialize<'de, D>(de: D) -> Result<DateTime<Local>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(de)?;
        let naive = NaiveDateTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)?;
        Ok(Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| serde::de::Error::custom("invalid local time"))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn serializes_local_time() {
        let when = Local.with_ymd_and_hms(2024, 1, 1, 8, 0, 0).unwrap();
        let s = Sensation::<String> {
            kind: "heartbeat".into(),
            when,
            what: "test".into(),
            source: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("2024-01-01 08:00:00"));
    }
}
