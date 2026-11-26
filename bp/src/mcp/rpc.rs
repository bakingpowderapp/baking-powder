use super::{Reply, catalogue, object, text};
use sdk::json::{self, Json};

const PROTOCOL: &str = "2025-06-18";
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;

pub(super) fn respond(body: &str, asking: bool, forward: &dyn Fn(&str, &Json) -> Reply) -> Option<Json> {
    let Some(request) = json::parse(body) else {
        return Some(failure(Json::Null, PARSE_ERROR, "The body is not JSON"));
    };
    if request.as_array().is_some() {
        return Some(failure(Json::Null, INVALID_REQUEST, "One request at a time; batches are not taken"));
    }
    let id = request.get("id").cloned().filter(|id| !matches!(id, Json::Null));
    let method = request.get("method").and_then(Json::as_str).unwrap_or("");
    let params = request.get("params");
    let result = match method {
        "initialize" => initialized(params, asking, forward),
        "ping" => object(Vec::new()),
        "tools/list" => object(vec![("tools", catalogue::listed())]),
        "tools/call" => called(params, forward),
        _ if method.starts_with("notifications/") => return None,
        _ => return id.map(|id| failure(id, METHOD_NOT_FOUND, &format!("No method named {method}"))),
    };
    let id = id?;
    Some(object(vec![("jsonrpc", text("2.0")), ("id", id), ("result", result)]))
}

fn initialized(params: Option<&Json>, asking: bool, forward: &dyn Fn(&str, &Json) -> Reply) -> Json {
    let version = params.and_then(|params| params.get("protocolVersion")).and_then(Json::as_str).unwrap_or(PROTOCOL);
    forward("initialize", params.and_then(|params| params.get("clientInfo")).unwrap_or(&Json::Null));
    object(vec![
        ("protocolVersion", text(version)),
        ("capabilities", object(vec![("tools", object(vec![("listChanged", Json::Bool(false))]))])),
        ("serverInfo", object(vec![("name", text("Baking Powder")), ("version", text(env!("CARGO_PKG_VERSION")))])),
        ("instructions", Json::String(catalogue::instructions(asking))),
    ])
}

fn called(params: Option<&Json>, forward: &dyn Fn(&str, &Json) -> Reply) -> Json {
    let name = params.and_then(|params| params.get("name")).and_then(Json::as_str).unwrap_or("");
    let args = params.and_then(|params| params.get("arguments")).cloned().unwrap_or(Json::Object(Vec::new()));
    let reply = if catalogue::known(name) { forward(name, &args) } else { Reply::refused(format!("No tool named {name}")) };
    let content = std::iter::once(reply.text).chain(reply.aside).map(|said| object(vec![("type", text("text")), ("text", Json::String(said))]));
    object(vec![("content", Json::Array(content.collect())), ("isError", Json::Bool(reply.failed))])
}

fn failure(id: Json, code: i64, message: &str) -> Json {
    object(vec![("jsonrpc", text("2.0")), ("id", id), ("error", object(vec![("code", Json::Number(code as f64)), ("message", text(message))]))])
}
