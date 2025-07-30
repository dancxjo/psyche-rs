use crate::store::FileStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: Option<String>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    pub id: Option<Value>,
}

#[derive(Serialize)]
pub struct RpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub id: Option<Value>,
}

#[derive(Deserialize)]
struct MemorizeParams {
    kind: String,
    data: Value,
}

#[derive(Deserialize)]
struct ListParams {
    kind: String,
}

#[derive(Deserialize)]
struct QueryVectorParams {
    kind: String,
    vector: Vec<f32>,
    top_k: usize,
}

/// Dispatch a single JSON-RPC request.
pub async fn dispatch(req: RpcRequest, store: &FileStore) -> anyhow::Result<RpcResponse> {
    match req.method.as_str() {
        "ping" | "status" => Ok(RpcResponse {
            jsonrpc: "2.0",
            result: Some(json!("ok")),
            error: None,
            id: req.id,
        }),
        "memorize" => {
            let params: MemorizeParams = serde_json::from_value(req.params)?;
            store.append(&params.kind, &params.data).await?;
            Ok(RpcResponse {
                jsonrpc: "2.0",
                result: Some(Value::Bool(true)),
                error: None,
                id: req.id,
            })
        }
        "list" => {
            let params: ListParams = serde_json::from_value(req.params)?;
            let entries = store.list(&params.kind).await?;
            Ok(RpcResponse {
                jsonrpc: "2.0",
                result: Some(Value::Array(entries)),
                error: None,
                id: req.id,
            })
        }
        "query_vector" => {
            let params: QueryVectorParams = serde_json::from_value(req.params)?;
            let hits = store
                .query_vector(&params.kind, &params.vector, params.top_k)
                .await?;
            Ok(RpcResponse {
                jsonrpc: "2.0",
                result: Some(Value::Array(hits)),
                error: None,
                id: req.id,
            })
        }
        "query_graph" | "episode" => Ok(RpcResponse {
            jsonrpc: "2.0",
            result: Some(Value::Null),
            error: None,
            id: req.id,
        }),
        m => Ok(RpcResponse {
            jsonrpc: "2.0",
            result: None,
            error: Some(format!("unknown method {m}")),
            id: req.id,
        }),
    }
}
