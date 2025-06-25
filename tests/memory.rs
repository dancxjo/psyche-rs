use psyche_rs::{Memory, Sensation};
use serde_json::json;
use std::time::SystemTime;
use uuid::Uuid;

#[test]
fn memory_uuid_and_timestamp() {
    let s = Sensation {
        uuid: Uuid::new_v4(),
        kind: "test".into(),
        from: "tester".into(),
        payload: json!({"a":1}),
        timestamp: SystemTime::now(),
    };
    let m = Memory::Sensation(s.clone());
    assert_eq!(m.uuid(), s.uuid);
    assert_eq!(m.timestamp().unwrap(), s.timestamp);
    let opt: Option<&Sensation> = m.try_as();
    assert!(opt.is_none());
}
