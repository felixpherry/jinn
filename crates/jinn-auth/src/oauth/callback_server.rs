//! A single-purpose local HTTP listener for OAuth redirect callbacks.
//!
//! The listener exists only for the duration of one login attempt. It accepts
//! the provider's redirect, validates the `state` parameter, shows the user a
//! short confirmation page, and hands the authorization code back to the flow.
//!
//! Binding can fail (another process may already hold the port). That is not
//! fatal: browser login also accepts a pasted code, so the flow continues
//! without a listener.

use error_stack::{Report, ResultExt as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use wherror::Error;

use crate::interaction::CancelSignal;

/// Raised when the callback listener cannot serve an authorization code.
#[derive(Debug, Error)]
pub enum CallbackError {
    /// The local port could not be bound.
    #[error("could not listen for the authorization callback")]
    Bind,
    /// The attempt was cancelled while waiting.
    #[error("authentication cancelled")]
    Cancelled,
    /// The listener failed while waiting for the redirect.
    #[error("authorization callback failed")]
    Accept,
}

/// Largest redirect request jinn will read. Redirects are a single short GET;
/// anything larger is not a callback worth parsing.
const MAX_REQUEST_BYTES: usize = 8192;

/// A bound listener waiting for one OAuth redirect.
#[derive(Debug)]
pub struct CallbackListener {
    listener: TcpListener,
}

impl CallbackListener {
    /// Binds `addr`, ready to accept the provider's redirect.
    ///
    /// # Errors
    ///
    /// Returns an error if the address is already in use or cannot be bound.
    pub async fn bind(addr: &str) -> Result<Self, Report<CallbackError>> {
        let listener = TcpListener::bind(addr)
            .await
            .change_context(CallbackError::Bind)
            .attach(format!("failed to bind {addr}"))?;
        Ok(Self { listener })
    }

    /// Waits for a redirect to `path` carrying `expected_state`, and returns
    /// its authorization code.
    ///
    /// Requests to other paths, or carrying a mismatched `state`, are answered
    /// with an error page and ignored; the listener keeps waiting.
    ///
    /// # Errors
    ///
    /// Returns an error if the attempt is cancelled or the listener fails.
    pub async fn accept_code(
        &self,
        path: &str,
        expected_state: &str,
        cancel: &CancelSignal,
    ) -> Result<String, Report<CallbackError>> {
        loop {
            let stream = tokio::select! {
                accepted = self.listener.accept() => accepted
                    .change_context(CallbackError::Accept)?
                    .0,
                () = cancel.cancelled() => return Err(Report::new(CallbackError::Cancelled)),
            };

            let handled = tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(Report::new(CallbackError::Cancelled)),
                result = tokio::time::timeout(std::time::Duration::from_secs(5), handle_connection(stream, path, expected_state)) => result,
            };
            let Ok(handled) = handled else { continue };
            match handled {
                Ok(Some(code)) => return Ok(code),
                Ok(None) => {}
                Err(err) => {
                    tracing::debug!(error = %err, "ignoring malformed authorization callback");
                }
            }
        }
    }
}

/// Reads one request, answers it, and returns the authorization code when the
/// request was the callback jinn is waiting for.
async fn handle_connection(
    mut stream: TcpStream,
    path: &str,
    expected_state: &str,
) -> Result<Option<String>, Report<CallbackError>> {
    let request = read_request_line(&mut stream).await?;
    let Some(target) = request_target(&request) else {
        respond(&mut stream, 400, &error_page("Malformed request.")).await;
        return Ok(None);
    };

    let (request_path, query) = split_target(target);
    if request_path != path {
        respond(&mut stream, 404, &error_page("Unexpected callback path.")).await;
        return Ok(None);
    }

    let params: Vec<(String, String)> = form_urlencoded_pairs(query);
    let value = |key: &str| {
        params
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    };

    if value("state").as_deref() != Some(expected_state) {
        respond(
            &mut stream,
            400,
            &error_page("Authorization state mismatch."),
        )
        .await;
        return Ok(None);
    }

    let Some(code) = value("code").filter(|code| !code.is_empty()) else {
        respond(&mut stream, 400, &error_page("Missing authorization code.")).await;
        return Ok(None);
    };

    respond(&mut stream, 200, &success_page()).await;
    Ok(Some(code))
}

/// Reads bytes until the end of the request head, then returns the first line.
async fn read_request_line(stream: &mut TcpStream) -> Result<String, Report<CallbackError>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .change_context(CallbackError::Accept)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(chunk.get(..read).unwrap_or_default());
        let text = String::from_utf8_lossy(&buffer);
        if text.contains("\r\n") || text.contains('\n') || buffer.len() >= MAX_REQUEST_BYTES {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buffer).into_owned();
    Ok(text.lines().next().unwrap_or_default().to_owned())
}

/// Extracts the request target from a `GET /path?query HTTP/1.1` line.
fn request_target(request_line: &str) -> Option<&str> {
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    (method == "GET").then_some(target)
}

/// Splits a request target into its path and query string.
fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

/// Decodes an `application/x-www-form-urlencoded` query string.
fn form_urlencoded_pairs(query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

/// Writes a minimal HTTP response and closes the connection.
async fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if let Err(err) = stream.write_all(response.as_bytes()).await {
        tracing::debug!(error = %err, "failed to write authorization callback response");
    }
    let _ = stream.shutdown().await;
}

/// The page shown after a successful authorization.
fn success_page() -> String {
    page(
        "Authentication complete",
        "You can close this window and return to jinn.",
    )
}

/// The page shown when a callback could not be accepted.
fn error_page(detail: &str) -> String {
    page("Authentication failed", detail)
}

/// Renders a minimal self-contained HTML page.
fn page(title: &str, detail: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head>\
         <body style=\"font-family:system-ui,sans-serif;padding:3rem;text-align:center\">\
         <h1>{title}</h1><p>{detail}</p></body></html>"
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    async fn bound_listener() -> (CallbackListener, u16) {
        let listener = CallbackListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let port = listener.listener.local_addr().expect("local addr").port();
        (listener, port)
    }

    async fn get(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        stream
            .write_all(format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .expect("write request");
        let mut response = String::new();
        let mut buffer = [0_u8; 1024];
        while let Ok(read) = stream.read(&mut buffer).await {
            if read == 0 {
                break;
            }
            response.push_str(&String::from_utf8_lossy(
                buffer.get(..read).unwrap_or_default(),
            ));
        }
        response
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_matching_callback_yields_its_authorization_code() {
        // Given a listener waiting for a callback.
        let (listener, port) = bound_listener().await;
        let cancel = CancelSignal::new();
        let waiting = tokio::spawn(async move {
            listener
                .accept_code("/auth/callback", "state-1", &cancel)
                .await
        });

        // When the provider redirects with a matching state.
        let _ = get(port, "/auth/callback?code=the-code&state=state-1").await;

        // Then the authorization code is handed back.
        let code = waiting.await.expect("task").expect("code received");
        assert_eq!(code, "the-code");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_state_mismatch_does_not_complete_the_login() {
        // Given a listener waiting for a callback.
        let (listener, port) = bound_listener().await;
        let cancel = CancelSignal::new();
        let cancel_handle = cancel.clone();
        let waiting = tokio::spawn(async move {
            listener
                .accept_code("/auth/callback", "state-1", &cancel)
                .await
        });

        // When a callback arrives carrying a different state.
        let response = get(port, "/auth/callback?code=the-code&state=attacker").await;

        // Then it is refused and the flow keeps waiting.
        assert!(
            response.contains("400"),
            "mismatched state must be refused: {response}"
        );
        cancel_handle.cancel();
        let result = waiting.await.expect("task");
        assert!(result.is_err(), "login must not complete on state mismatch");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn cancelling_stops_the_listener() {
        // Given a listener waiting for a callback.
        let (listener, _port) = bound_listener().await;
        let cancel = CancelSignal::new();
        let cancel_handle = cancel.clone();
        let waiting = tokio::spawn(async move {
            listener
                .accept_code("/auth/callback", "state-1", &cancel)
                .await
        });

        // When the attempt is cancelled.
        cancel_handle.cancel();

        // Then the listener stops waiting.
        let result = waiting.await.expect("task");
        assert!(result.is_err(), "cancellation must end the wait");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn binding_an_address_already_in_use_fails() {
        // Given a listener already holding a port.
        let (held, port) = bound_listener().await;

        // When binding the same port again.
        let result = CallbackListener::bind(&format!("127.0.0.1:{port}")).await;

        // Then binding fails so the caller can fall back to manual entry.
        assert!(result.is_err(), "the second bind must fail");
        drop(held);
    }
}
