use chrono::Local;
use futures::{FutureExt, StreamExt};
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, error, trace, warn};
use xmlparser::{ElementEnd, Token as XmlToken, Tokenizer};

use crate::llm::types::{Token, TokenStream};
use crate::{Action, Intention, Sensation};

fn try_start_tag(buf: &str) -> Option<(usize, String, Map<String, Value>, bool)> {
    if !buf.starts_with('<') {
        return None;
    }
    let gt = buf.find('>')? + 1;
    let segment = &buf[..gt];
    let mut tokenizer = Tokenizer::from_fragment(segment, 0..segment.len());
    let mut tag = None;
    let mut attrs = Map::new();
    let mut self_close = false;
    let mut saw_end = false;

    for tok in tokenizer {
        match tok.ok()? {
            XmlToken::ElementStart { local, .. } => {
                tag = Some(local.as_str().to_ascii_lowercase());
            }
            XmlToken::Attribute { local, value, .. } => {
                attrs.insert(
                    local.as_str().to_string(),
                    Value::String(value.as_str().to_string()),
                );
            }
            XmlToken::ElementEnd { end, .. } => {
                self_close = matches!(end, ElementEnd::Empty);
                saw_end = true;
                break;
            }
            _ => {}
        }
    }

    if saw_end {
        tag.map(|t| (gt, t, attrs, self_close))
    } else {
        None
    }
}

pub async fn drive_llm_stream<T>(
    name: &str,
    mut stream: TokenStream,
    window: Arc<Mutex<Vec<Sensation<T>>>>,
    tx: UnboundedSender<Vec<Intention>>,
    thoughts_tx: Option<UnboundedSender<Vec<Sensation<String>>>>,
) where
    T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    let start = std::time::Instant::now();
    debug!(agent = %name, "LLM request START {:?}", start);
    let mut buf = String::new();
    let mut full_text = String::new();
    let mut state: Option<(String, String, String, UnboundedSender<String>)> = None;
    let mut pending_text = String::new();
    let mut shutdown = Box::pin(crate::shutdown_signal()).fuse();

    loop {
        tokio::select! {
            tok = stream.next() => {
                match tok {
                    Some(Token { text }) => {
                        trace!(token = %text, "Will received LLM token");
                        buf.push_str(&text);
                        full_text.push_str(&text);
                    }
                    None => break,
                }
            }
            _ = &mut shutdown => {
                warn!("Will LLM stream interrupted");
                break;
            }
        }

        parse_buffer(
            &mut buf,
            &mut state,
            &mut pending_text,
            &window,
            &thoughts_tx,
            &tx,
        );
    }

    flush_pending(&mut pending_text, &window, &thoughts_tx);
    debug!(agent = %name, %full_text, "llm full response");
    debug!(agent = %name, "LLM request END {:?}", std::time::Instant::now());
    debug!(agent = %name, "LLM call ended");
    trace!("will llm stream finished");
}

fn parse_buffer<T>(
    buf: &mut String,
    state: &mut Option<(String, String, String, UnboundedSender<String>)>,
    pending_text: &mut String,
    window: &Arc<Mutex<Vec<Sensation<T>>>>,
    thoughts_tx: &Option<UnboundedSender<Vec<Sensation<String>>>>,
    tx: &UnboundedSender<Vec<Intention>>,
) where
    T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    loop {
        if let Some((_, closing, closing_lower, tx_body)) = state.as_ref() {
            if let Some(pos) = buf.to_ascii_lowercase().find(closing_lower) {
                if pos > 0 {
                    let prefix = crate::safe_prefix(buf, pos);
                    let _ = tx_body.send(prefix.to_string());
                }
                let drain_len = crate::safe_prefix(buf, pos + closing.len()).len();
                buf.drain(..drain_len);
                *state = None;
                continue;
            } else if buf.len() > closing.len() {
                let send_len = buf.len() - closing.len();
                let prefix = crate::safe_prefix(buf, send_len);
                let _ = tx_body.send(prefix.to_string());
                buf.drain(..prefix.len());
                break;
            } else {
                // Wait for more data before deciding if the closing tag starts
                break;
            }
        }

        if let Some((consumed, tag, map, self_close)) = try_start_tag(buf) {
            if !pending_text.trim().is_empty() {
                if let Ok(what) =
                    serde_json::from_value::<T>(Value::String(pending_text.trim().to_string()))
                {
                    let sensation = Sensation {
                        kind: "thought".into(),
                        when: Local::now(),
                        what,
                        source: None,
                    };
                    window.lock().unwrap().push(sensation);
                }
                if let Some(tx) = thoughts_tx {
                    let s = Sensation {
                        kind: "thought".into(),
                        when: Local::now(),
                        what: format!("I thought to myself: {}", pending_text.trim()),
                        source: None,
                    };
                    let _ = tx.send(vec![s]);
                }
                pending_text.clear();
            }

            let closing = format!("</{}>", tag);
            let closing_lower = closing.to_ascii_lowercase();

            let _ = buf.drain(..consumed);

            let (btx, brx) = unbounded_channel();
            let action = Action::new(
                tag.clone(),
                Value::Object(map),
                UnboundedReceiverStream::new(brx).boxed(),
            );
            let intention = Intention::to(action).assign(tag.clone());

            debug!(motor_name = %tag, "Will assigned motor on intention");
            debug!(?intention, "Will built intention");

            let val = serde_json::to_value(&intention).unwrap();
            let what = serde_json::from_value(val).unwrap_or_default();
            window.lock().unwrap().push(Sensation {
                kind: "intention".into(),
                when: Local::now(),
                what,
                source: None,
            });

            let _ = tx.send(vec![intention]);
            if !self_close {
                *state = Some((tag, closing, closing_lower, btx));
            }
        } else if let Some(idx) = buf.find('<') {
            let prefix = crate::safe_prefix(buf, idx);
            pending_text.push_str(prefix);
            buf.drain(..prefix.len());
            break;
        } else {
            if !buf.is_empty() {
                pending_text.push_str(&buf);
            }
            buf.clear();
            break;
        }
    }
}

fn flush_pending<T>(
    pending_text: &mut String,
    window: &Arc<Mutex<Vec<Sensation<T>>>>,
    thoughts_tx: &Option<UnboundedSender<Vec<Sensation<String>>>>,
) where
    T: Clone + Default + Send + 'static + serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    if pending_text.trim().is_empty() {
        return;
    }
    if let Ok(what) = serde_json::from_value::<T>(Value::String(pending_text.trim().to_string())) {
        let sensation = Sensation {
            kind: "thought".into(),
            when: Local::now(),
            what,
            source: None,
        };
        window.lock().unwrap().push(sensation);
    }
    if let Some(tx) = thoughts_tx {
        let s = Sensation {
            kind: "thought".into(),
            when: Local::now(),
            what: format!("I thought to myself: {}", pending_text.trim()),
            source: None,
        };
        let _ = tx.send(vec![s]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn stream_bodies_include_initial_chunks() {
        let tokens = vec![
            Token {
                text: "<log>".into(),
            },
            Token { text: "he".into() },
            Token { text: "llo".into() },
            Token {
                text: "</log>".into(),
            },
        ];
        let stream = Box::pin(stream::iter(tokens));
        let window = Arc::new(Mutex::new(Vec::<Sensation<String>>::new()));
        let (tx, mut rx) = unbounded_channel();

        drive_llm_stream("test", stream, window.clone(), tx, None).await;
        let mut intentions = rx.recv().await.expect("intentions");
        assert_eq!(intentions.len(), 1);
        let mut action = intentions.pop().unwrap().action;
        let text = action.collect_text().await;
        assert_eq!(text, "hello");
    }
}
#[cfg(test)]
mod _parse_speak_log_tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn parses_speak_and_log_intentions() {
        let text = "<speak speaker_id=\"p234\" language_id=\"en\">hi</speak><log>done</log>";
        let tokens = text
            .chars()
            .map(|c| Token {
                text: c.to_string(),
            })
            .collect::<Vec<_>>();
        let stream = Box::pin(stream::iter(tokens));
        let window = Arc::new(Mutex::new(Vec::<Sensation<String>>::new()));
        let (tx, mut rx) = unbounded_channel();

        drive_llm_stream("test", stream, window.clone(), tx, None).await;
        let mut all = Vec::new();
        while let Some(mut batch) = rx.recv().await {
            all.append(&mut batch);
        }
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].assigned_motor, "speak");
        assert_eq!(all[1].assigned_motor, "log");
    }

    #[tokio::test]
    async fn parses_self_closing_intention() {
        let text = "<read_log_memory/>";
        let tokens = text
            .chars()
            .map(|c| Token {
                text: c.to_string(),
            })
            .collect::<Vec<_>>();
        let stream = Box::pin(stream::iter(tokens));
        let window = Arc::new(Mutex::new(Vec::<Sensation<String>>::new()));
        let (tx, mut rx) = unbounded_channel();

        drive_llm_stream("test", stream, window.clone(), tx, None).await;
        let mut intentions = rx.recv().await.expect("intentions");
        assert_eq!(intentions.len(), 1);
        let mut int = intentions.pop().unwrap();
        assert_eq!(int.assigned_motor, "read_log_memory");
        let body = int.action.collect_text().await;
        assert!(body.is_empty());
    }
}
