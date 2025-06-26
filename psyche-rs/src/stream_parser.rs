use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Map, Value};

use crate::action::Action;

/// Result of parsing a streamed XML action.
///
/// This holds the parsed [`Action`] with any attributes as well as the
/// captured body text.
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedAction {
    pub action: Action,
    pub body: String,
}

/// Parse a single streamed action represented as XML.
///
/// # Examples
///
/// ```
/// use psyche_rs::stream_parser::{parse_streamed_action, ParsedAction};
/// use serde_json::json;
///
/// let xml = "<say pitch=\"low\">hi</say>";
/// let parsed = parse_streamed_action(xml).unwrap();
/// assert_eq!(parsed.action.name, "say");
/// assert_eq!(parsed.action.attributes["pitch"], json!("low"));
/// assert_eq!(parsed.body, "hi");
/// ```
pub fn parse_streamed_action(xml: &str) -> Option<ParsedAction> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();

    let mut action_name = None;
    let mut attributes = Map::new();
    let mut body = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                action_name = Some(String::from_utf8_lossy(e.name().as_ref()).into_owned());
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                    let value = attr.unescape_value().unwrap_or_default().to_string();
                    attributes.insert(key, Value::String(value));
                }
            }
            Ok(Event::Text(e)) => {
                body.push_str(&e.unescape().unwrap_or_default());
            }
            Ok(Event::Eof) | Ok(Event::End(_)) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    Some(ParsedAction {
        action: Action {
            name: action_name?,
            attributes: Value::Object(attributes),
        },
        body,
    })
}
