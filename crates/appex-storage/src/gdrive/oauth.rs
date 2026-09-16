//! OAuth 2.0 installed-app flow with loopback redirect and PKCE.

use std::time::Duration;

use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::api::{fetch_about, token_request};
use super::{DRIVE_FILE_SCOPE, GoogleEndpoints};
use crate::util::{base64url, random_bytes};
use crate::{Result, StorageError};

/// Maximum size of the redirect request read from the browser.
const MAX_REQUEST_BYTES: usize = 16 * 1024;
/// How long a single browser connection may stay silent (browsers pre-open sockets).
const CONNECTION_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Desktop OAuth client. Google does not treat a desktop client secret as
/// confidential, but its token endpoint still requires it for this client type.
#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: Option<SecretString>,
}

#[derive(Debug, Clone)]
pub struct AuthorizedAccount {
    pub refresh_token: SecretString,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

/// A started authorization: a loopback listener is bound and waiting.
pub struct PendingAuthorization {
    http: Client,
    client: OAuthClient,
    endpoints: GoogleEndpoints,
    listener: TcpListener,
    redirect_uri: String,
    state: String,
    code_verifier: SecretString,
    authorize_url: Url,
}

impl std::fmt::Debug for PendingAuthorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAuthorization").field("redirect_uri", &self.redirect_uri).finish_non_exhaustive()
    }
}

/// What the browser sent to the loopback listener.
struct Callback {
    target: String,
    reply: oneshot::Sender<(u16, &'static str)>,
}

enum CallbackOutcome {
    /// Not the redirect (e.g. `/favicon.ico`).
    Ignore,
    /// Redirect with a missing or wrong `state`: rejected, keep waiting.
    Rejected,
    Denied(String),
    Code(String),
}

impl PendingAuthorization {
    /// Binds `127.0.0.1:0`, generates PKCE verifier + state and builds the consent URL
    /// (`access_type=offline`, `prompt=consent`, scope `drive.file`).
    pub async fn start(http: Client, client: OAuthClient) -> Result<Self> {
        Self::start_with_endpoints(http, client, GoogleEndpoints::default()).await
    }

    pub async fn start_with_endpoints(http: Client, client: OAuthClient, endpoints: GoogleEndpoints) -> Result<Self> {
        if client.client_id.trim().is_empty() {
            return Err(StorageError::NotConfigured("Google OAuth client id is empty".into()));
        }

        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}");

        let code_verifier = base64url(&random_bytes(32)?);
        let code_challenge = base64url(&Sha256::digest(code_verifier.as_bytes()));
        let state = base64url(&random_bytes(24)?);

        let mut authorize_url = endpoints.auth_url.clone();
        authorize_url
            .query_pairs_mut()
            .append_pair("client_id", &client.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", DRIVE_FILE_SCOPE)
            .append_pair("code_challenge", &code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");

        Ok(Self {
            http,
            client,
            endpoints,
            listener,
            redirect_uri,
            state,
            code_verifier: SecretString::from(code_verifier),
            authorize_url,
        })
    }

    /// URL to open in the system browser.
    pub fn authorize_url(&self) -> &Url {
        &self.authorize_url
    }

    /// Loopback redirect URI registered in the consent URL.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Waits for the redirect, validates `state`, exchanges the code and reads the
    /// account email (`about.get`). Answers the browser with a small HTML page.
    pub async fn finish(self, cancel: CancellationToken, timeout: Duration) -> Result<AuthorizedAccount> {
        let deadline = tokio::time::Instant::now() + timeout;
        let (tx, mut rx) = mpsc::channel::<Callback>(8);

        loop {
            tokio::select! {
                () = cancel.cancelled() => return Err(StorageError::Cancelled),
                () = tokio::time::sleep_until(deadline) => return Err(StorageError::Timeout),
                accepted = self.listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => { tokio::spawn(serve_connection(stream, tx.clone())); }
                        Err(err) => tracing::debug!(error = %err, "oauth loopback accept failed"),
                    }
                }
                Some(callback) = rx.recv() => {
                    match self.evaluate(&callback.target) {
                        CallbackOutcome::Ignore => { let _ = callback.reply.send((404, PAGE_NOT_FOUND)); }
                        CallbackOutcome::Rejected => {
                            tracing::warn!("oauth redirect with invalid state rejected");
                            let _ = callback.reply.send((400, PAGE_INVALID));
                        }
                        CallbackOutcome::Denied(error) => {
                            let _ = callback.reply.send((200, PAGE_DENIED));
                            return Err(StorageError::AuthorizationDenied(error));
                        }
                        CallbackOutcome::Code(code) => {
                            let result = tokio::select! {
                                () = cancel.cancelled() => Err(StorageError::Cancelled),
                                result = tokio::time::timeout_at(deadline, self.exchange(&code)) => {
                                    result.unwrap_or(Err(StorageError::Timeout))
                                }
                            };
                            let page = if result.is_ok() { PAGE_SUCCESS } else { PAGE_ERROR };
                            let _ = callback.reply.send((200, page));
                            return result;
                        }
                    }
                }
            }
        }
    }

    fn evaluate(&self, target: &str) -> CallbackOutcome {
        let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
            return CallbackOutcome::Ignore;
        };
        if url.path() != "/" {
            return CallbackOutcome::Ignore;
        }
        let mut state = None;
        let mut code = None;
        let mut error = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "state" => state = Some(value.into_owned()),
                "code" => code = Some(value.into_owned()),
                "error" => error = Some(value.into_owned()),
                _ => {}
            }
        }
        if state.is_none() && code.is_none() && error.is_none() {
            return CallbackOutcome::Ignore;
        }
        if state.as_deref() != Some(self.state.as_str()) {
            return CallbackOutcome::Rejected;
        }
        match (code, error) {
            (_, Some(error)) => CallbackOutcome::Denied(error),
            (Some(code), None) if !code.is_empty() => CallbackOutcome::Code(code),
            _ => CallbackOutcome::Rejected,
        }
    }

    async fn exchange(&self, code: &str) -> Result<AuthorizedAccount> {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", self.client.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code_verifier", self.code_verifier.expose_secret()),
        ];
        if let Some(secret) = &self.client.client_secret {
            form.push(("client_secret", secret.expose_secret()));
        }
        let tokens = match token_request(&self.http, &self.endpoints.token_url, &form).await {
            // An invalid authorization code is a failed authorization, not an expired account.
            Err(StorageError::AuthExpired) => {
                return Err(StorageError::AuthorizationDenied("invalid_grant: authorization code rejected".into()));
            }
            other => other?,
        };
        let refresh_token = tokens.refresh_token.filter(|t| !t.is_empty()).ok_or_else(|| {
            StorageError::AuthorizationDenied(
                "Google returned no refresh token; remove the app at https://myaccount.google.com/permissions and connect again"
                    .into(),
            )
        })?;

        let (email, display_name) = match fetch_about(&self.http, &self.endpoints.drive_api, &tokens.access_token).await
        {
            Ok(user) => user,
            Err(err) => {
                tracing::warn!(error = %err, "could not read Google account info after authorization");
                (None, None)
            }
        };

        Ok(AuthorizedAccount { refresh_token: SecretString::from(refresh_token), email, display_name })
    }
}

/// Reads one HTTP request line from the browser, hands it to the flow and writes
/// the page it chooses.
async fn serve_connection(mut stream: TcpStream, tx: mpsc::Sender<Callback>) {
    let mut buf = Vec::with_capacity(1024);
    let read = tokio::time::timeout(CONNECTION_READ_TIMEOUT, async {
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= MAX_REQUEST_BYTES {
                break;
            }
        }
        Ok::<_, std::io::Error>(())
    })
    .await;
    if !matches!(read, Ok(Ok(()))) || buf.is_empty() {
        return;
    }

    let head = String::from_utf8_lossy(&buf);
    let mut parts = head.lines().next().unwrap_or_default().split_whitespace();
    let (status, page) = match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) if target.starts_with('/') => {
            let (reply_tx, reply_rx) = oneshot::channel();
            if tx.send(Callback { target: target.to_owned(), reply: reply_tx }).await.is_err() {
                return;
            }
            match reply_rx.await {
                Ok(answer) => answer,
                Err(_) => return,
            }
        }
        _ => (400, PAGE_INVALID),
    };

    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

const PAGE_SUCCESS: &str = "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>Appex Backup</title></head>\
<body style=\"font-family:sans-serif;padding:3rem\"><h1>Appex Backup: autorización completada, puedes cerrar esta ventana.</h1></body></html>";

const PAGE_DENIED: &str = "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>Appex Backup</title></head>\
<body style=\"font-family:sans-serif;padding:3rem\"><h1>Appex Backup: la autorización fue cancelada. Puedes cerrar esta ventana.</h1></body></html>";

const PAGE_ERROR: &str = "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>Appex Backup</title></head>\
<body style=\"font-family:sans-serif;padding:3rem\"><h1>Appex Backup: no se pudo completar la autorización.</h1><p>Vuelve a la aplicación para ver el detalle.</p></body></html>";

const PAGE_INVALID: &str = "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>Appex Backup</title></head>\
<body style=\"font-family:sans-serif;padding:3rem\"><h1>Appex Backup: solicitud de autorización no válida.</h1></body></html>";

const PAGE_NOT_FOUND: &str = "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\"><title>Appex Backup</title></head><body>No encontrado</body></html>";
