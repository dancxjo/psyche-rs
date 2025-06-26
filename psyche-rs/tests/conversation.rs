use llm::chat::ChatRole;
use psyche_rs::conversation::{Conversation, Role};

#[test]
fn prompt_includes_system_and_tail() {
    const PROMPT: &str = "You are Pete";
    let mut c = Conversation::new(PROMPT.into(), 10);
    c.hear(Role::Interlocutor, "hi there");
    c.hear(Role::Me, "hello");
    c.hear(Role::Interlocutor, "how are you");
    c.hear(Role::Me, "great");

    let prompt = c.to_prompt();
    assert_eq!(prompt.first().unwrap().content, PROMPT);
    assert_eq!(prompt[1].role, ChatRole::User);
    assert_eq!(prompt[1].content, "hi there");
    assert_eq!(prompt[2].role, ChatRole::Assistant);
    assert_eq!(prompt[2].content, "hello");
    assert_eq!(prompt.len(), 5);
}

#[test]
fn tail_prunes_excess_tokens() {
    let mut c = Conversation::new("sys".into(), 3);
    for i in 0..5 {
        c.hear(Role::Interlocutor, format!("m{}", i));
    }
    let tail = c.tail();
    assert_eq!(tail.len(), 3);
    assert_eq!(tail[0].1, "m2");
    assert_eq!(tail[1].1, "m3");
    assert_eq!(tail[2].1, "m4");
}
