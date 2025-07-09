use async_trait::async_trait;
use chrono::Local;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;
use tracing::debug;
use url::Url;

use psyche_rs::{ActionResult, Completion, Intention, Motor, MotorError, Sensation};

/// Motor that assigns a name to a detected face.
///
/// The `identify` action links a face node to a `Person` in Neo4j and emits
/// a `face.named` sensation.
pub struct IdentifyMotor {
    client: reqwest::Client,
    neo4j_url: Url,
    neo_user: String,
    neo_pass: String,
    tx: UnboundedSender<Vec<Sensation<serde_json::Value>>>,
}

impl IdentifyMotor {
    /// Create a new motor configured with Neo4j credentials.
    pub fn new(
        client: reqwest::Client,
        neo4j_url: Url,
        neo_user: impl Into<String>,
        neo_pass: impl Into<String>,
        tx: UnboundedSender<Vec<Sensation<serde_json::Value>>>,
    ) -> Self {
        Self {
            client,
            neo4j_url,
            neo_user: neo_user.into(),
            neo_pass: neo_pass.into(),
            tx,
        }
    }

    async fn link_face(&self, face_id: &str, name: &str) -> Result<(), MotorError> {
        let query = "MERGE (f:FaceEmbedding {uuid:$face})\nSET f.name=$name\nMERGE (p:Person {name:$name})\nMERGE (f)-[:REPRESENTS]->(p)";
        let params = json!({"face": face_id, "name": name});
        let payload = json!({"statements":[{"statement":query,"parameters":params}]});
        let url = self
            .neo4j_url
            .join("db/neo4j/tx/commit")
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        let resp = self
            .client
            .post(url)
            .basic_auth(&self.neo_user, Some(&self.neo_pass))
            .json(&payload)
            .send()
            .await
            .map_err(|e| MotorError::Failed(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            Err(MotorError::Failed(format!("neo4j error {status}: {text}")))
        }
    }
}

#[async_trait]
impl Motor for IdentifyMotor {
    fn description(&self) -> &'static str {
        "Associate a name with a detected face.\n\
Parameters: `face` and `name`.\n\
Example:\n\
<identify face=\"abc_face0\" name=\"Travis\"/>\n\
Explanation:\n\
Links the face node to a Person node in Neo4j and emits a `face.named` sensation."
    }

    fn name(&self) -> &'static str {
        "identify"
    }

    async fn perform(&self, intention: Intention) -> Result<ActionResult, MotorError> {
        if intention.action.name != "identify" {
            return Err(MotorError::Unrecognized);
        }
        let action = intention.action;
        let face_id = action
            .params
            .get("face")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MotorError::Failed("missing face".into()))?
            .to_string();
        let name = action
            .params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MotorError::Failed("missing name".into()))?
            .to_string();
        self.link_face(&face_id, &name).await?;
        let completion = Completion::of_action(action);
        debug!(?completion, "action completed");
        let sensation = Sensation {
            kind: "face.named".into(),
            when: Local::now(),
            what: json!({"face_id": face_id, "name": name}),
            source: None,
        };
        let _ = self.tx.send(vec![sensation.clone()]);
        Ok(ActionResult {
            sensations: vec![sensation],
            completed: true,
            completion: Some(completion),
            interruption: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, stream};
    use httpmock::prelude::*;
    use psyche_rs::{Action, Intention, Motor};
    use serde_json::Value;
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn links_face_and_emits_sensation() {
        let neo = MockServer::start();
        let neo_mock = neo.mock(|when, then| {
            when.method(POST).path("/db/neo4j/tx/commit");
            then.status(200);
        });
        let (tx, mut rx) = unbounded_channel::<Vec<Sensation<serde_json::Value>>>();
        let motor = IdentifyMotor::new(
            reqwest::Client::new(),
            Url::parse(&neo.url("")).unwrap(),
            "u",
            "p",
            tx,
        );
        let body = stream::empty().boxed();
        let mut params = serde_json::Map::new();
        params.insert("face".into(), Value::String("f1".into()));
        params.insert("name".into(), Value::String("Travis".into()));
        let action = Action::new("identify", Value::Object(params), body);
        let intent = Intention::to(action).assign("identify");
        let res = motor.perform(intent).await.unwrap();
        assert!(res.completed);
        let batch = rx.recv().await.unwrap();
        assert_eq!(batch[0].kind, "face.named");
        assert_eq!(batch[0].what["name"], "Travis");
        neo_mock.assert();
    }
}
