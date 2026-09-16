//! JSON-2 client (Odoo ≥ 19): `POST /json/2/<model>/<method>`,
//! `Authorization: bearer <api key>`, `X-Odoo-Database: <db>`.

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use secrecy::ExposeSecret;
use serde_json::{Map, Value};
use tokio::sync::OnceCell;
use url::Url;

use crate::net::{endpoint, map_reqwest, read_text_limited};
use crate::rpc::{Credentials, MODEL_NOT_FOUND, OdooRpc, RpcProtocol, SecretKind, is_missing_model_message};
use crate::{OdooError, Result};

pub(crate) const DATABASE_HEADER: &str = "X-Odoo-Database";

pub(crate) struct Json2Client {
    http: Client,
    base_url: Url,
    database: String,
    credentials: Credentials,
    uid: OnceCell<i64>,
}

impl Json2Client {
    pub(crate) fn new(http: Client, base_url: Url, database: String, credentials: Credentials) -> Self {
        Self { http, base_url, database, credentials, uid: OnceCell::new() }
    }

    async fn post(&self, model: &str, method: &str, body: Value) -> Result<Value> {
        if self.credentials.kind != SecretKind::ApiKey {
            return Err(OdooError::UnsupportedProtocol {
                protocol: RpcProtocol::Json2.label().into(),
                reason: "JSON-2 requires an API key".into(),
            });
        }
        let url = endpoint(&self.base_url, &format!("json/2/{model}/{method}"))?;
        let mut request = self.http.post(url).bearer_auth(self.credentials.secret.expose_secret()).json(&body);
        if !self.database.is_empty() {
            request = request.header(DATABASE_HEADER, &self.database);
        }
        let response = request.send().await.map_err(map_reqwest)?;
        let status = response.status();
        if status.is_success() {
            return response.json::<Value>().await.map_err(|err| OdooError::Protocol(err.to_string()));
        }
        let text = read_text_limited(response, 16 * 1024).await;
        Err(error_from_status(status, &text, model))
    }
}

/// Maps a JSON-2 error response. The body is `{"name", "message", "arguments", "context", "debug"}`.
pub(crate) fn error_from_status(status: StatusCode, body: &str, model: &str) -> OdooError {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("message"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_owned());
    let name = parsed.as_ref().and_then(|v| v.get("name")).and_then(Value::as_str).unwrap_or_default();

    match status {
        StatusCode::UNAUTHORIZED => OdooError::AuthenticationFailed,
        StatusCode::FORBIDDEN => OdooError::AccessDenied(message),
        StatusCode::NOT_FOUND if is_missing_model_message(&message, model) => {
            OdooError::Rpc { code: MODEL_NOT_FOUND.into(), message }
        }
        StatusCode::NOT_FOUND => OdooError::Rpc { code: "not_found".into(), message },
        _ if name.contains("AccessDenied") => OdooError::AuthenticationFailed,
        _ if name.contains("AccessError") => OdooError::AccessDenied(message),
        _ if parsed.is_some() => {
            let code = if name.contains("UserError") || name.contains("ValidationError") {
                "user_error"
            } else {
                "application_error"
            };
            OdooError::Rpc { code: code.into(), message }
        }
        _ => OdooError::HttpStatus { status: status.as_u16(), message },
    }
}

#[async_trait]
impl OdooRpc for Json2Client {
    fn protocol(&self) -> RpcProtocol {
        RpcProtocol::Json2
    }

    fn database(&self) -> &str {
        &self.database
    }

    async fn authenticate(&self) -> Result<i64> {
        self.uid
            .get_or_try_init(|| async {
                // `res.users.context_get` is public on every version and includes `uid`.
                let context = self.post("res.users", "context_get", Value::Object(Map::new())).await?;
                context
                    .get("uid")
                    .and_then(Value::as_i64)
                    .filter(|uid| *uid > 0)
                    .ok_or_else(|| OdooError::Protocol(format!("context_get returned no uid: {context}")))
            })
            .await
            .copied()
    }

    async fn call(&self, model: &str, method: &str, ids: &[i64], kwargs: Map<String, Value>) -> Result<Value> {
        let mut body = kwargs;
        if !ids.is_empty() {
            body.insert("ids".into(), Value::from(ids.to_vec()));
        }
        self.post(model, method, Value::Object(body)).await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn maps_error_statuses() {
        assert!(matches!(error_from_status(StatusCode::UNAUTHORIZED, "{}", "x"), OdooError::AuthenticationFailed));
        let forbidden = json!({"name": "odoo.exceptions.AccessError", "message": "nope"}).to_string();
        assert!(
            matches!(error_from_status(StatusCode::FORBIDDEN, &forbidden, "x"), OdooError::AccessDenied(m) if m == "nope")
        );

        let missing =
            json!({"name": "werkzeug.exceptions.NotFound", "message": "the model 'obd.backup.api' does not exist"});
        let err = error_from_status(StatusCode::NOT_FOUND, &missing.to_string(), "obd.backup.api");
        assert!(err.is_model_not_found());

        let other_404 = error_from_status(StatusCode::NOT_FOUND, "<html>404</html>", "obd.backup.api");
        assert!(matches!(other_404, OdooError::Rpc { ref code, .. } if code == "not_found"));

        let user_error = json!({"name": "odoo.exceptions.UserError", "message": "Boom"}).to_string();
        assert!(matches!(
            error_from_status(StatusCode::UNPROCESSABLE_ENTITY, &user_error, "x"),
            OdooError::Rpc { code, message } if code == "user_error" && message == "Boom"
        ));
        assert!(matches!(
            error_from_status(StatusCode::BAD_GATEWAY, "<html>bad gateway</html>", "x"),
            OdooError::HttpStatus { status: 502, .. }
        ));
    }
}
