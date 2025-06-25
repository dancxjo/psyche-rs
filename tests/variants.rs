use psyche_rs::codec::serialize_memory;
use psyche_rs::{
    Completion, Impression, Intention, IntentionStatus, Interruption, Memory, Sensation, Urge,
};
use serde_json::json;
use std::time::UNIX_EPOCH;
use uuid::Uuid;

fn sample_sensation() -> Sensation {
    Sensation {
        uuid: Uuid::new_v4(),
        kind: "vision".into(),
        from: "eye".into(),
        payload: json!({"light":true}),
        timestamp: UNIX_EPOCH,
    }
}

fn sample_impression(source: &Sensation) -> Impression {
    Impression {
        uuid: Uuid::new_v4(),
        how: "see".into(),
        topic: "object".into(),
        composed_of: vec![source.uuid],
        timestamp: UNIX_EPOCH,
    }
}

fn sample_urge(source: &Sensation) -> Urge {
    Urge {
        uuid: Uuid::new_v4(),
        source: source.uuid,
        motor_name: "move".into(),
        parameters: json!({"dir":"forward"}),
        intensity: 0.5,
        timestamp: UNIX_EPOCH,
    }
}

fn sample_intention(urge: &Urge) -> Intention {
    Intention {
        uuid: Uuid::new_v4(),
        urge: urge.uuid,
        motor_name: urge.motor_name.clone(),
        parameters: urge.parameters.clone(),
        issued_at: UNIX_EPOCH,
        resolved_at: None,
        status: IntentionStatus::Pending,
    }
}

fn sample_completion(intention: &Intention) -> Completion {
    Completion {
        uuid: Uuid::new_v4(),
        intention: intention.uuid,
        outcome: "done".into(),
        transcript: None,
        timestamp: UNIX_EPOCH,
    }
}

fn sample_interruption(intention: &Intention) -> Interruption {
    Interruption {
        uuid: Uuid::new_v4(),
        intention: intention.uuid,
        reason: "stop".into(),
        timestamp: UNIX_EPOCH,
    }
}

#[test]
fn serialize_all_variants() {
    let sens = sample_sensation();
    let imp = sample_impression(&sens);
    let urge = sample_urge(&sens);
    let intent = sample_intention(&urge);
    let comp = sample_completion(&intent);
    let intr = sample_interruption(&intent);

    let cases = vec![
        Memory::Sensation(sens.clone()),
        Memory::Impression(imp),
        Memory::Urge(urge.clone()),
        Memory::Intention(intent.clone()),
        Memory::Completion(comp),
        Memory::Interruption(intr),
    ];

    for memory in cases {
        let (label, map) = serialize_memory(&memory).unwrap();
        match memory {
            Memory::Sensation(_) => assert_eq!(label, "Sensation"),
            Memory::Impression(_) => assert_eq!(label, "Impression"),
            Memory::Urge(_) => assert_eq!(label, "Urge"),
            Memory::Intention(_) => assert_eq!(label, "Intention"),
            Memory::Completion(_) => assert_eq!(label, "Completion"),
            Memory::Interruption(_) => assert_eq!(label, "Interruption"),
            Memory::Of(_) => unreachable!(),
        }
        assert!(map.get("uuid").is_some());
    }
}

#[test]
fn uuid_and_timestamp_for_variants() {
    let sens = sample_sensation();
    let imp = sample_impression(&sens);
    let urge = sample_urge(&sens);
    let intent = sample_intention(&urge);
    let comp = sample_completion(&intent);
    let intr = sample_interruption(&intent);
    let cases = vec![
        (Memory::Sensation(sens.clone()), sens.uuid),
        (Memory::Impression(imp.clone()), imp.uuid),
        (Memory::Urge(urge.clone()), urge.uuid),
        (Memory::Intention(intent.clone()), intent.uuid),
        (Memory::Completion(comp.clone()), comp.uuid),
        (Memory::Interruption(intr.clone()), intr.uuid),
    ];

    for (mem, id) in cases {
        assert_eq!(mem.uuid(), id);
        assert_eq!(mem.timestamp().unwrap(), UNIX_EPOCH);
    }
}
