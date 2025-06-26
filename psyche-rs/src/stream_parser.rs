use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Map, Value};
use tracing::{debug, error};

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

/// Parses a streamed action in XML format into a [`ParsedAction`].
///
/// Logs suspicious input structure and any parsing errors. Returns `None`
/// if parsing fails or the input is malformed.
///
/// Example input: `<say pitch="low">hi</say>`
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

    // Detect malformed or suspiciously complex input (e.g., multiple roots)
    if xml.matches('<').count() > 1 && !xml.contains("</") {
        debug!(%xml, "Input may contain multiple root elements or malformed structure");
    }

    let mut closed = false;
    let mut depth = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if depth == 0 {
                    action_name = Some(String::from_utf8_lossy(e.name().as_ref()).into_owned());
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                        let value = attr.unescape_value().unwrap_or_default().to_string();
                        attributes.insert(key, Value::String(value));
                    }
                }
                depth += 1;
            }
            Ok(Event::Text(e)) => {
                body.push_str(&e.unescape().unwrap_or_default());
            }
            Ok(Event::End(_)) => {
                if depth == 0 {
                    error!(%xml, "Unexpected closing tag while parsing streamed action");
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    closed = true;
                    break;
                }
            }
            Ok(Event::Eof) => {
                error!(%xml, "Unexpected EOF while parsing streamed action");
                return None;
            }
            Err(e) => {
                error!(%xml, ?e, "Failed to parse streamed action");
                return None;
            }
            _ => {}
        }
        buf.clear();
    }

    if !closed {
        return None;
    }

    Some(ParsedAction {
        action: Action {
            name: action_name?,
            attributes: Value::Object(attributes),
        },
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_malformed_stream_logs_and_fails() {
        let xml = r#"<say pitch="high">Hello<do>Oops</do>"#;
        let result = parse_streamed_action(xml);
        assert!(result.is_none());
    }
}
