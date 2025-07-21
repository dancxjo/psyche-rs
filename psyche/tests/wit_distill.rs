use psyche::models::Sensation;
use psyche::wit::distill;

#[test]
fn distills_basic_feeling() {
    let s = Sensation {
        id: "1".into(),
        path: "/chat".into(),
        text: "I feel lonely".into(),
    };
    let instant = distill(&s).expect("should distill");
    assert_eq!(instant.how, "The interlocutor feels lonely");
    assert_eq!(instant.what, vec!["1".to_string()]);
}
