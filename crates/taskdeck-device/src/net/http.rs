//! One HTTP client over `ureq` (device.md §10.1 to §10.3, §10.10): no proxy,
//! no redirects, no cookies, no `Referer` (DEV-NET-003); exactly the headers
//! of §10.2 (DEV-NET-004); the timeouts of §10.10 (DEV-NET-015). The relay
//! origin is `https://<host>`; with `test-trust`, a loopback host is reached
//! over `http` (§11.1 item 1), and any other host over `https` as always.

use std::sync::Arc;
use std::time::Duration;

use ureq::config::AutoHeaderValue;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use zeroize::Zeroizing;

use super::wire;

/// The `User-Agent` of every relay request (device.md §10.2).
pub(crate) const USER_AGENT: &str = "TaskDeck-Sync/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    Get,
    Post,
    Put,
    Delete,
}

/// The credential of a request, in the forms of relay.md §5.6.
pub(crate) enum Auth {
    Token(Zeroizing<String>),
    Door([u8; 5]),
    Create(Zeroizing<String>),
    /// The token the transport holds for the request's host (resolved by
    /// the transport before the request is sent).
    Stored,
}

/// One relay request.
pub(crate) struct Req {
    pub(crate) method: Method,
    pub(crate) host: String,
    /// The path and query, e.g. `/v1/b/<board>/hello`.
    pub(crate) path: String,
    pub(crate) auth: Auth,
    /// `X-TD-Proof`: the ticket key (relay.md §10.8).
    pub(crate) proof: Option<Zeroizing<[u8; 32]>>,
    pub(crate) body: Option<Vec<u8>>,
    /// The largest answer body read.
    pub(crate) limit: usize,
}

/// An answer.
pub(crate) struct Resp {
    pub(crate) status: u16,
    /// `X-TD-Relay` is present (relay.md §4.2, §14.5).
    pub(crate) relay: bool,
    /// `Content-Type: application/json`.
    pub(crate) json: bool,
    pub(crate) body: Vec<u8>,
}

/// No answer: DNS, TCP, TLS, a timeout or a body that could not be read
/// (device.md §10.6 first row); or a host this build may not reach
/// (`device/insecure_url`, DEV-NET-001).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fail {
    Unreachable,
    Insecure,
}

pub(crate) struct Client {
    agent: ureq::Agent,
}

impl Client {
    pub(crate) fn new() -> Client {
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::PlatformVerifier)
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .proxy(None)
            .max_redirects(0)
            .user_agent(USER_AGENT)
            .accept(AutoHeaderValue::None)
            .accept_encoding("identity")
            .tls_config(tls)
            .timeout_resolve(Some(Duration::from_millis(10_000)))
            .timeout_connect(Some(Duration::from_millis(10_000)))
            .timeout_recv_response(Some(Duration::from_millis(20_000)))
            .timeout_send_body(Some(Duration::from_millis(20_000 + 16_000)))
            .timeout_recv_body(Some(Duration::from_millis(20_000 + 16_000)))
            .build()
            .new_agent();
        Client { agent }
    }

    /// Performs one request and reads its answer.
    pub(crate) fn perform(&self, req: &Req) -> Result<Resp, Fail> {
        let url = format!("{}{}", origin_for(&req.host)?, req.path);
        let auth: Zeroizing<String> = Zeroizing::new(match &req.auth {
            Auth::Token(t) => format!("Bearer {}", t.as_str()),
            Auth::Door(d) => format!("Door {}", crate::strict_json::hex(d)),
            Auth::Create(c) => format!("Create {}", c.as_str()),
            Auth::Stored => return Err(Fail::Unreachable),
        });
        let proof = req.proof.as_ref().map(|p| Zeroizing::new(crate::strict_json::hex(&p[..])));
        let result = match req.method {
            Method::Get | Method::Delete => {
                let mut b = if req.method == Method::Get { self.agent.get(&url) } else { self.agent.delete(&url) };
                b = b.header("Authorization", auth.as_str());
                if let Some(p) = &proof {
                    b = b.header("X-TD-Proof", p.as_str());
                }
                b.call()
            }
            Method::Post | Method::Put => {
                let mut b = if req.method == Method::Post { self.agent.post(&url) } else { self.agent.put(&url) };
                b = b.header("Authorization", auth.as_str()).header("Content-Type", "application/octet-stream");
                if let Some(p) = &proof {
                    b = b.header("X-TD-Proof", p.as_str());
                }
                b.send(req.body.as_deref().unwrap_or(&[]))
            }
        };
        let mut resp = result.map_err(|_| Fail::Unreachable)?;
        let status = resp.status().as_u16();
        let relay = resp.headers().get("x-td-relay").is_some();
        let json = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(';').next().is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json")));
        let body = resp
            .body_mut()
            .with_config()
            .limit(req.limit as u64)
            .read_to_vec()
            .map_err(|_| Fail::Unreachable)?;
        Ok(Resp { status, relay, json, body })
    }
}

/// `https://<host>`, or with `test-trust` `http://<host>` for a loopback
/// host (device.md §11.1 item 1). DEV-TRUST-004 holds without the feature:
/// every relay is reached over TLS.
pub(crate) fn origin_for(host: &str) -> Result<String, Fail> {
    let origin = taskdeck_seal::relay_origin(host).map_err(|_| Fail::Insecure)?;
    #[cfg(feature = "test-trust")]
    if wire::is_loopback(host) {
        return Ok(format!("http://{host}"));
    }
    #[cfg(not(feature = "test-trust"))]
    let _ = wire::is_loopback;
    Ok(origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEV-NET-001, DEV-TRUST-004: a relay is reached over `https`; only
    /// `test-trust` reaches a loopback host over `http` (§11.1 item 1).
    #[test]
    fn origins() {
        assert_eq!(origin_for("relay.example").as_deref(), Ok("https://relay.example"));
        #[cfg(feature = "test-trust")]
        assert_eq!(origin_for("127.0.0.1:8080").as_deref(), Ok("http://127.0.0.1:8080"));
        #[cfg(not(feature = "test-trust"))]
        assert_eq!(origin_for("127.0.0.1:8080").as_deref(), Ok("https://127.0.0.1:8080"));
        assert_eq!(origin_for("Bad Host"), Err(Fail::Insecure));
    }
}
