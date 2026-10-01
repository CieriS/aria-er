use std::fmt;
use std::thread;
use std::time::Duration;

use aq_core::SourceError;
use tracing::warn;

use crate::HttpConfig;

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

impl UreqTransport {
    pub fn new(config: &HttpConfig) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(config.connect_timeout_secs))
            .timeout(Duration::from_secs(config.timeout_secs))
            .build();
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
            Ok(response) => response.into_string().map_err(|e| TransportError {
                message: format!("reading response body: {e}"),
                retryable: true,
            }),
            Err(ureq::Error::Status(code, _)) => Err(TransportError {
                message: format!("HTTP status {code}"),
                retryable: code == 429 || code >= 500,
            }),
            Err(ureq::Error::Transport(e)) => Err(TransportError {
                message: e.to_string(),
                retryable: true,
            }),
        }
    }
}

/// GET with exponential backoff on retryable failures.
pub(crate) fn get_with_retry<T: Transport>(
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
