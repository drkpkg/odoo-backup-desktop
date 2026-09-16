//! Minimal XML-RPC client for Odoo (supports `<nil/>`, which Odoo sends with allow_none).
//!
//! Values are mapped to/from `serde_json::Value`: `dateTime.iso8601` and `base64`
//! decode as strings. Faults on `/xmlrpc/2/*` carry Odoo's integer codes:
//! 1 application error (faultString is a traceback), 2 warning/UserError,
//! 3 AccessDenied, 4 AccessError.

use std::fmt::Write as _;

use async_trait::async_trait;
use quick_xml::Reader;
use quick_xml::events::Event;
use reqwest::Client;
use secrecy::ExposeSecret;
use serde_json::{Map, Number, Value, json};
use tokio::sync::OnceCell;
use url::Url;

use crate::net::{endpoint, map_reqwest, read_text_limited};
use crate::rpc::{Credentials, MODEL_NOT_FOUND, OdooRpc, RpcProtocol, is_missing_model_message};
use crate::{OdooError, Result};

const FAULT_APPLICATION_ERROR: i64 = 1;
const FAULT_WARNING: i64 = 2;
const FAULT_ACCESS_DENIED: i64 = 3;
const FAULT_ACCESS_ERROR: i64 = 4;

pub(crate) struct XmlRpcClient {
    http: Client,
    base_url: Url,
    database: String,
    credentials: Credentials,
    uid: OnceCell<i64>,
}

impl XmlRpcClient {
    pub(crate) fn new(http: Client, base_url: Url, database: String, credentials: Credentials) -> Self {
        Self { http, base_url, database, credentials, uid: OnceCell::new() }
    }
}

#[async_trait]
impl OdooRpc for XmlRpcClient {
    fn protocol(&self) -> RpcProtocol {
        RpcProtocol::XmlRpc
    }

    fn database(&self) -> &str {
        &self.database
    }

    async fn authenticate(&self) -> Result<i64> {
        self.uid
            .get_or_try_init(|| async {
                let params = [
                    json!(self.database),
                    json!(self.credentials.login),
                    json!(self.credentials.secret.expose_secret()),
                    json!({}),
                ];
                match call(&self.http, &self.base_url, "common", "authenticate", &params).await? {
                    Value::Number(n) if n.as_i64().is_some_and(|uid| uid > 0) => Ok(n.as_i64().unwrap_or_default()),
                    Value::Bool(false) => Err(OdooError::AuthenticationFailed),
                    other => Err(OdooError::Protocol(format!("unexpected authenticate result: {other}"))),
                }
            })
            .await
            .copied()
    }

    async fn call(&self, model: &str, method: &str, ids: &[i64], kwargs: Map<String, Value>) -> Result<Value> {
        let uid = self.authenticate().await?;
        let args = if ids.is_empty() { json!([]) } else { json!([ids]) };
        let params = [
            json!(self.database),
            json!(uid),
            json!(self.credentials.secret.expose_secret()),
            json!(model),
            json!(method),
            args,
            Value::Object(kwargs),
        ];
        match call(&self.http, &self.base_url, "object", "execute_kw", &params).await {
            Err(OdooError::Rpc { message, .. }) if is_missing_model_message(&message, model) => {
                Err(OdooError::Rpc { code: MODEL_NOT_FOUND.into(), message })
            }
            other => other,
        }
    }
}

/// Calls `service.method(params)` on `/xmlrpc/2/<service>`.
pub(crate) async fn call(
    http: &Client,
    base_url: &Url,
    service: &str,
    method: &str,
    params: &[Value],
) -> Result<Value> {
    let url = endpoint(base_url, &format!("xmlrpc/2/{service}"))?;
    let body = encode_call(method, params);
    let response = http
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "text/xml; charset=utf-8")
        .body(body)
        .send()
        .await
        .map_err(map_reqwest)?;

    let status = response.status();
    if !status.is_success() {
        let text = read_text_limited(response, 4096).await;
        return Err(OdooError::HttpStatus { status: status.as_u16(), message: summarize(&text) });
    }
    let text = response.text().await.map_err(map_reqwest)?;
    match decode_response(&text)? {
        XmlRpcResponse::Success(value) => Ok(value),
        XmlRpcResponse::Fault { code, message } => Err(fault_to_error(&code, &message)),
    }
}

fn summarize(text: &str) -> String {
    let trimmed = text.trim();
    let mut short: String = trimmed.chars().take(200).collect();
    if trimmed.chars().count() > 200 {
        short.push('…');
    }
    short
}

/// Maps an Odoo fault to an error. Unknown/extended codes become `Rpc`.
pub(crate) fn fault_to_error(code: &Value, message: &str) -> OdooError {
    let code_number = code.as_i64().or_else(|| code.as_str().and_then(|s| s.trim().parse().ok()));
    match code_number {
        Some(FAULT_ACCESS_DENIED) => OdooError::AuthenticationFailed,
        Some(FAULT_ACCESS_ERROR) => OdooError::AccessDenied(message.trim().to_owned()),
        Some(FAULT_WARNING) => OdooError::Rpc { code: "user_error".into(), message: message.trim().to_owned() },
        Some(FAULT_APPLICATION_ERROR) => {
            let last = last_traceback_line(message);
            if is_missing_database_message(&last) {
                OdooError::Rpc { code: "database_not_found".into(), message: last }
            } else {
                OdooError::Rpc { code: "application_error".into(), message: last }
            }
        }
        _ => {
            // Historical `/xmlrpc/` string codes such as "AccessDenied".
            let code_text = code.as_str().unwrap_or_default();
            if code_text == "AccessDenied" {
                OdooError::AuthenticationFailed
            } else {
                OdooError::Rpc { code: "fault".into(), message: last_traceback_line(message) }
            }
        }
    }
}

/// Odoo puts the whole traceback in faultString for application errors; the last
/// non-empty line holds `ExceptionType: message`.
fn last_traceback_line(message: &str) -> String {
    message.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or_default().to_owned()
}

fn is_missing_database_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("database") && lower.contains("does not exist")
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

pub(crate) fn encode_call(method: &str, params: &[Value]) -> String {
    let mut out = String::with_capacity(256);
    out.push_str("<?xml version=\"1.0\"?><methodCall><methodName>");
    out.push_str(&quick_xml::escape::escape(method));
    out.push_str("</methodName><params>");
    for param in params {
        out.push_str("<param>");
        encode_value(param, &mut out);
        out.push_str("</param>");
    }
    out.push_str("</params></methodCall>");
    out
}

fn encode_value(value: &Value, out: &mut String) {
    out.push_str("<value>");
    match value {
        Value::Null => out.push_str("<nil/>"),
        Value::Bool(b) => {
            let _ = write!(out, "<boolean>{}</boolean>", u8::from(*b));
        }
        Value::Number(n) => encode_number(n, out),
        Value::String(s) => {
            out.push_str("<string>");
            out.push_str(&quick_xml::escape::escape(s.as_str()));
            out.push_str("</string>");
        }
        Value::Array(items) => {
            out.push_str("<array><data>");
            for item in items {
                encode_value(item, out);
            }
            out.push_str("</data></array>");
        }
        Value::Object(members) => {
            out.push_str("<struct>");
            for (name, member) in members {
                out.push_str("<member><name>");
                out.push_str(&quick_xml::escape::escape(name.as_str()));
                out.push_str("</name>");
                encode_value(member, out);
                out.push_str("</member>");
            }
            out.push_str("</struct>");
        }
    }
    out.push_str("</value>");
}

fn encode_number(n: &Number, out: &mut String) {
    if let Some(i) = n.as_i64() {
        if i32::try_from(i).is_ok() {
            let _ = write!(out, "<int>{i}</int>");
        } else {
            let _ = write!(out, "<i8>{i}</i8>");
        }
    } else if let Some(f) = n.as_f64() {
        let _ = write!(out, "<double>{f}</double>");
    }
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum XmlRpcResponse {
    Success(Value),
    Fault { code: Value, message: String },
}

#[derive(Debug, Default)]
struct Node {
    name: String,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
}

pub(crate) fn decode_response(xml: &str) -> Result<XmlRpcResponse> {
    let root = parse_tree(xml)?;
    if root.name != "methodResponse" {
        return Err(protocol(format!("expected <methodResponse>, got <{}>", root.name)));
    }
    if let Some(fault) = root.child("fault") {
        let value = fault.child("value").ok_or_else(|| protocol("<fault> without <value>"))?;
        let decoded = decode_value(value)?;
        let code = decoded.get("faultCode").cloned().unwrap_or(Value::Null);
        let message = decoded.get("faultString").and_then(Value::as_str).unwrap_or_default().to_owned();
        return Ok(XmlRpcResponse::Fault { code, message });
    }
    let value = root
        .child("params")
        .and_then(|p| p.child("param"))
        .and_then(|p| p.child("value"))
        .ok_or_else(|| protocol("response without params/param/value"))?;
    Ok(XmlRpcResponse::Success(decode_value(value)?))
}

fn protocol(message: impl Into<String>) -> OdooError {
    OdooError::Protocol(format!("invalid XML-RPC response: {}", message.into()))
}

fn decode_value(node: &Node) -> Result<Value> {
    let Some(typed) = node.children.first() else {
        // `<value>text</value>` without a type element is a string.
        return Ok(Value::String(node.text.clone()));
    };
    let text = typed.text.as_str();
    match typed.name.as_str() {
        "string" => Ok(Value::String(typed.text.clone())),
        "int" | "i4" | "i8" | "i1" | "i2" | "bigint" => text
            .trim()
            .parse::<i64>()
            .map(|i| Value::Number(i.into()))
            .map_err(|_| protocol(format!("invalid integer {text:?}"))),
        "boolean" => match text.trim() {
            "1" | "true" => Ok(Value::Bool(true)),
            "0" | "false" => Ok(Value::Bool(false)),
            other => Err(protocol(format!("invalid boolean {other:?}"))),
        },
        "double" | "float" | "bigdecimal" => text
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| protocol(format!("invalid double {text:?}"))),
        "nil" => Ok(Value::Null),
        "dateTime.iso8601" | "base64" => Ok(Value::String(text.trim().to_owned())),
        "array" => {
            let data = typed.child("data").ok_or_else(|| protocol("<array> without <data>"))?;
            data.children
                .iter()
                .filter(|c| c.name == "value")
                .map(decode_value)
                .collect::<Result<Vec<_>>>()
                .map(Value::Array)
        }
        "struct" => {
            let mut map = Map::new();
            for member in typed.children.iter().filter(|c| c.name == "member") {
                let name = member.child("name").ok_or_else(|| protocol("<member> without <name>"))?;
                let value = member.child("value").ok_or_else(|| protocol("<member> without <value>"))?;
                map.insert(name.text.clone(), decode_value(value)?);
            }
            Ok(Value::Object(map))
        }
        other => Err(protocol(format!("unsupported value type <{other}>"))),
    }
}

fn parse_tree(xml: &str) -> Result<Node> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;

    let attach = |node: Node, stack: &mut Vec<Node>, root: &mut Option<Node>| match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => *root = Some(node),
    };

    loop {
        match reader.read_event().map_err(|err| protocol(err.to_string()))? {
            Event::Start(start) => {
                let name = start.local_name().as_ref().to_owned();
                stack.push(Node { name, ..Node::default() });
            }
            Event::Empty(start) => {
                let name = start.local_name().as_ref().to_owned();
                attach(Node { name, ..Node::default() }, &mut stack, &mut root);
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(|| protocol("unbalanced end tag"))?;
                attach(node, &mut stack, &mut root);
            }
            Event::Text(text) => {
                if let Some(current) = stack.last_mut() {
                    current.text.push_str(&text.xml10_content());
                }
            }
            Event::CData(data) => {
                if let Some(current) = stack.last_mut() {
                    current.text.push_str(&data.into_inner());
                }
            }
            Event::GeneralRef(reference) => {
                let name = reference.into_inner();
                let resolved = resolve_entity(&name).ok_or_else(|| protocol(format!("unknown entity &{name};")))?;
                if let Some(current) = stack.last_mut() {
                    current.text.push(resolved);
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) | Event::DocType(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(protocol("unexpected end of document"));
    }
    root.ok_or_else(|| protocol("empty document"))
}

fn resolve_entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                name.strip_prefix('#')?.parse().ok()?
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_value(value: &Value) -> Value {
        let mut body = String::from("<?xml version='1.0'?><methodResponse><params><param>");
        encode_value(value, &mut body);
        body.push_str("</param></params></methodResponse>");
        match decode_response(&body).unwrap() {
            XmlRpcResponse::Success(v) => v,
            fault => panic!("unexpected {fault:?}"),
        }
    }

    #[test]
    fn encodes_call_with_all_types() {
        let xml = encode_call(
            "execute_kw",
            &[
                json!("db & <x>"),
                json!(7),
                json!(5_000_000_000_i64),
                json!(true),
                json!(null),
                json!(1.5),
                json!([1, "a"]),
                json!({"k": "v"}),
            ],
        );
        assert!(xml.starts_with("<?xml version=\"1.0\"?><methodCall><methodName>execute_kw</methodName>"));
        assert!(xml.contains("<string>db &amp; &lt;x&gt;</string>"));
        assert!(xml.contains("<int>7</int>"));
        assert!(xml.contains("<i8>5000000000</i8>"));
        assert!(xml.contains("<boolean>1</boolean>"));
        assert!(xml.contains("<nil/>"));
        assert!(xml.contains("<double>1.5</double>"));
        assert!(
            xml.contains("<array><data><value><int>1</int></value><value><string>a</string></value></data></array>")
        );
        assert!(xml.contains("<struct><member><name>k</name><value><string>v</string></value></member></struct>"));
    }

    #[test]
    fn values_roundtrip() {
        let value = json!({
            "id": 42, "big": 9_000_000_000_i64, "neg": -3, "ok": false, "ratio": 0.25,
            "none": null, "name": "Compañía <A&B> \"q\" 'a'", "list": [1, [2, 3], {"x": null}], "empty": ""
        });
        assert_eq!(roundtrip_value(&value), value);
    }

    #[test]
    fn decodes_python_style_response() {
        let xml = r#"<?xml version='1.0'?>
<methodResponse>
<params>
<param>
<value><struct>
<member>
<name>server_version</name>
<value><string>17.0-20240101</string></value>
</member>
<member>
<name>server_version_info</name>
<value><array><data>
<value><int>17</int></value>
<value><int>0</int></value>
<value><int>0</int></value>
<value><string>final</string></value>
<value><int>0</int></value>
<value><string></string></value>
</data></array></value>
</member>
<member>
<name>untyped</name>
<value>plain &amp; text&#33;&#x3F;</value>
</member>
<member>
<name>when</name>
<value><dateTime.iso8601>20260916T10:00:00</dateTime.iso8601></value>
</member>
<member>
<name>nothing</name>
<value><nil/></value>
</member>
</struct></value>
</param>
</params>
</methodResponse>
"#;
        let XmlRpcResponse::Success(value) = decode_response(xml).unwrap() else { panic!("fault") };
        assert_eq!(value["server_version"], "17.0-20240101");
        assert_eq!(value["server_version_info"], json!([17, 0, 0, "final", 0, ""]));
        assert_eq!(value["untyped"], "plain & text!?");
        assert_eq!(value["when"], "20260916T10:00:00");
        assert_eq!(value["nothing"], Value::Null);
    }

    #[test]
    fn decodes_faults() {
        let xml = r#"<?xml version='1.0'?><methodResponse><fault><value><struct>
<member><name>faultCode</name><value><int>3</int></value></member>
<member><name>faultString</name><value><string>Access Denied</string></value></member>
</struct></value></fault></methodResponse>"#;
        let response = decode_response(xml).unwrap();
        assert_eq!(response, XmlRpcResponse::Fault { code: json!(3), message: "Access Denied".into() });
    }

    #[test]
    fn maps_fault_codes() {
        assert!(matches!(fault_to_error(&json!(3), "Access Denied"), OdooError::AuthenticationFailed));
        assert!(matches!(fault_to_error(&json!("AccessDenied"), "x"), OdooError::AuthenticationFailed));
        assert!(
            matches!(fault_to_error(&json!(4), "You are not allowed"), OdooError::AccessDenied(m) if m == "You are not allowed")
        );
        match fault_to_error(&json!(2), "Object obd.backup.api doesn't exist") {
            OdooError::Rpc { code, message } => {
                assert_eq!(code, "user_error");
                assert!(is_missing_model_message(&message, "obd.backup.api"));
            }
            other => panic!("unexpected {other:?}"),
        }
        let traceback = "Traceback (most recent call last):\n  File \"x.py\", line 1\nAttributeError: The method 'res.users.nope' does not exist\n";
        assert!(matches!(
            fault_to_error(&json!(1), traceback),
            OdooError::Rpc { code, message } if code == "application_error" && message == "AttributeError: The method 'res.users.nope' does not exist"
        ));
        assert!(matches!(
            fault_to_error(&json!(1), "Traceback\npsycopg2.OperationalError: connection failed: FATAL:  database \"nope\" does not exist"),
            OdooError::Rpc { code, .. } if code == "database_not_found"
        ));
    }

    #[test]
    fn rejects_malformed_documents() {
        assert!(decode_response("<methodResponse><params>").is_err());
        assert!(decode_response("<html><body>502</body></html>").is_err());
        assert!(
            decode_response(
                "<methodResponse><params><param><value><int>x</int></value></param></params></methodResponse>"
            )
            .is_err()
        );
    }
}
