//! HTTP transport shared by the sources: explicit timeouts and retry with backoff.

use std::fmt;
use std::thread;
use std::time::Duration;

use aq_core::SourceError;
use serde::Deserialize;
use tracing::warn;

/// HTTP timeouts and retry policy.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpConfig {
    pub connect_timeout_secs: u64,
    pub timeout_secs: u64,
    /// Total attempts per request, including the first one.
    pub max_attempts: u32,
    /// Delay before the first retry; doubles on every further retry.
    pub initial_backoff_ms: u64,
}

/// A failed HTTP exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    pub message: String,
    /// Whether the same request may succeed if repeated.
    pub retryable: bool,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Minimal HTTP GET, abstracted so the source can be tested without a network.
pub trait Transport {
    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, TransportError>;
}

/// Blocking HTTP transport with explicit timeouts.
pub struct UreqTransport {
    agent: ureq::Agent,
}

/// Largest response body accepted. The biggest real responses (a page of ARPAE
/// measurements, a year of weather for several locations) are below 10 MB.
const MAX_BODY_BYTES: u64 = 256 * 1024 * 1024;

impl UreqTransport {
    pub fn new(config: &HttpConfig) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(config.connect_timeout_secs)))
            .timeout_global(Some(Duration::from_secs(config.timeout_secs)))
            .build()
            .into();
        Self { agent }
    }
}

impl Transport for UreqTransport {
    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, TransportError> {
        let mut request = self.agent.get(url);
        for (name, value) in query {
            request = request.query(name, value);
        }
        match request.call() {
            Ok(mut response) => response
                .body_mut()
                .with_config()
                .limit(MAX_BODY_BYTES)
                .read_to_string()
                .map_err(|e| TransportError {
                    message: format!("reading response body: {e}"),
                    retryable: true,
                }),
            Err(ureq::Error::StatusCode(code)) => Err(TransportError {
                message: format!("HTTP status {code}"),
                retryable: code == 429 || code >= 500,
            }),
            // Timeouts, connection and protocol errors.
            Err(error) => Err(TransportError {
                message: error.to_string(),
                retryable: true,
            }),
        }
    }
}

/// GET with exponential backoff on retryable failures.
pub fn get_with_retry<T: Transport>(
    transport: &T,
    url: &str,
    query: &[(&str, &str)],
    policy: &HttpConfig,
) -> Result<String, SourceError> {
    let max_attempts = policy.max_attempts.max(1);
    let mut backoff = Duration::from_millis(policy.initial_backoff_ms);
    let mut attempt = 1;
    loop {
        match transport.get(url, query) {
            Ok(body) => return Ok(body),
            Err(error) if error.retryable && attempt < max_attempts => {
                warn!(
                    url,
                    attempt,
                    max_attempts,
                    backoff_ms = backoff.as_millis() as u64,
                    error = %error,
                    "request failed, retrying"
                );
                thread::sleep(backoff);
                backoff = backoff.saturating_mul(2);
                attempt += 1;
            }
            Err(error) => {
                return Err(SourceError::Transport {
                    url: url.to_owned(),
                    attempts: attempt,
                    message: error.message,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Scripted {
        responses: RefCell<Vec<Result<String, TransportError>>>,
        calls: RefCell<u32>,
    }

    impl Scripted {
        fn new(mut responses: Vec<Result<String, TransportError>>) -> Self {
            responses.reverse();
            Self {
                responses: RefCell::new(responses),
                calls: RefCell::new(0),
            }
        }
    }

    impl Transport for Scripted {
        fn get(&self, _url: &str, _query: &[(&str, &str)]) -> Result<String, TransportError> {
            *self.calls.borrow_mut() += 1;
            self.responses.borrow_mut().pop().unwrap()
        }
    }

    fn failure(retryable: bool) -> Result<String, TransportError> {
        Err(TransportError {
            message: "HTTP status 502".into(),
            retryable,
        })
    }

    fn policy(max_attempts: u32) -> HttpConfig {
        HttpConfig {
            connect_timeout_secs: 1,
            timeout_secs: 1,
            max_attempts,
            initial_backoff_ms: 0,
        }
    }

    #[test]
    fn retries_until_success() {
        let transport = Scripted::new(vec![failure(true), failure(true), Ok("ok".into())]);
        let body = get_with_retry(&transport, "http://x", &[], &policy(3)).unwrap();
        assert_eq!(body, "ok");
        assert_eq!(*transport.calls.borrow(), 3);
    }

    #[test]
    fn gives_up_after_max_attempts() {
        let transport = Scripted::new(vec![failure(true), failure(true), Ok("ok".into())]);
        let error = get_with_retry(&transport, "http://x", &[], &policy(2)).unwrap_err();
        assert!(matches!(error, SourceError::Transport { attempts: 2, .. }));
    }

    #[test]
    fn does_not_retry_permanent_failures() {
        let transport = Scripted::new(vec![failure(false), Ok("ok".into())]);
        let error = get_with_retry(&transport, "http://x", &[], &policy(5)).unwrap_err();
        assert!(matches!(error, SourceError::Transport { attempts: 1, .. }));
        assert_eq!(*transport.calls.borrow(), 1);
    }
}
