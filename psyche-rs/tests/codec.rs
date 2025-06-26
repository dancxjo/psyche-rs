use psyche_rs::codec::serialize_memory;
use psyche_rs::{Memory, Sensation};
use serde_json::json;
use std::time::SystemTime;
use uuid::Uuid;

#[test]
fn serialize_sensation() {
    let s = Sensation {
        uuid: Uuid::nil(),
        kind: "feel".into(),
        from: "tester".into(),
        payload: json!({"a":1}),
        timestamp: SystemTime::UNIX_EPOCH,
    };
    let m = Memory::Sensation(s);
    let (label, map) = serialize_memory(&m).unwrap();
    assert_eq!(label, "Sensation");
    assert_eq!(map.get("kind"), Some(&json!("feel")));
    assert_eq!(map.get("from"), Some(&json!("tester")));
}
