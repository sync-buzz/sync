//! The HTTP door, and what stands in it.
//!
//! A port is not a pipe. Anything running as this person can connect to it, and
//! so can a page in their browser — a site that knows the port can have the
//! browser POST to it, which is how local servers get read by strangers
//! (DNS rebinding). stdio had none of this surface, so everything here is the
//! price of the door rather than a feature of it:
//!
//! * the socket is bound to the loopback address and nothing else;
//! * every request carries a bearer token this process was started with;
//! * a request with an `Origin`, or with a `Host` that is not the loopback, is
//!   refused — both are browsers, and no MCP client sends either.
//!
//! None of the three is optional and none is the others' backup: the token
//! stops other processes, the `Origin` and `Host` checks stop the browser,
//! which would happily send a token it was told to send.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use rmcp::transport::StreamableHttpService;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;

use crate::server::SyncMcp;

/// Where an agent reaches this server.
pub const ENDPOINT: &str = "/mcp";

/// The environment variable the token arrives in.
///
/// Not a command-line argument: arguments are readable by every process on this
/// machine through `ps`, and a secret every process can read is the thing this
/// token exists to be instead of.
pub const TOKEN_VARIABLE: &str = sync_memory::SERVER_TOKEN_VARIABLE;

/// Serve `server` on `address`, for holders of `token`.
///
/// # Errors
///
/// Refuses an address that is not on the loopback or an empty token, and
/// reports whatever binding or serving the socket refused.
pub async fn serve(
    server: SyncMcp,
    address: SocketAddr,
    token: String,
) -> Result<(), Box<dyn std::error::Error>> {
    // Refused rather than corrected. `0.0.0.0` is one character away from
    // `127.0.0.1` and puts a machine's memory on its network; a server that
    // quietly narrowed it would be a server whose safety depends on nobody
    // reading the address they asked for.
    if !address.ip().is_loopback() {
        return Err(format!(
            "`{address}` is not a loopback address — this server is reachable from this machine \
             only, and binding it anywhere else would publish every project on it"
        )
        .into());
    }
    if token.trim().is_empty() {
        return Err(
            format!("{TOKEN_VARIABLE} is empty — the door does not open without one").into(),
        );
    }

    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        {
            let mut config = StreamableHttpServerConfig::default();
            // Plain JSON for the ordinary request-and-answer, with the server
            // free to fall back to an event stream when it has something to say
            // mid-call. Measured against a real client: Codex CLI talks to a
            // server answering `application/json` without asking for SSE.
            config.json_response = true;
            config
        },
    );

    let router =
        Router::new()
            .nest_service(ENDPOINT, service)
            .layer(middleware::from_fn_with_state(
                Arc::new(token),
                stands_in_the_door,
            ));

    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

/// Let through what is allowed through, and say nothing to the rest.
///
/// Every refusal is a bare status. A door that explained which of the checks a
/// request failed would be a door that helps whoever is trying them one at a
/// time.
async fn stands_in_the_door(
    State(token): State<Arc<String>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let headers = request.headers();
    if !from_this_machine(headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    if !carries(headers, &token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

/// Whether the request could have come from something that is not a browser.
///
/// An MCP client sends no `Origin`; a page always does, and cannot lie about
/// it. `Host` is the other half: a rebinding attack arrives with the attacker's
/// own name in it, having resolved that name to `127.0.0.1`.
#[must_use]
pub fn from_this_machine(headers: &HeaderMap) -> bool {
    if headers.contains_key(header::ORIGIN) {
        return false;
    }
    match headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
    {
        Some(host) => {
            let named = host.rsplit_once(':').map_or(host, |(name, _)| name);
            let named = named.trim_start_matches('[').trim_end_matches(']');
            named == "localhost"
                || named
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        }
        // HTTP/1.1 requires one and HTTP/2 supplies `:authority` in its place;
        // absent altogether there is nothing to check and nothing to trust.
        None => false,
    }
}

/// Whether the request carries this server's token.
///
/// Compared to the end regardless of where it first differs. A comparison that
/// stops at the first wrong byte tells the caller how much of the token it got
/// right, one request at a time.
#[must_use]
pub fn carries(headers: &HeaderMap, token: &str) -> bool {
    let Some(offered) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    let offered = offered.as_bytes();
    let token = token.as_bytes();
    let mut same = offered.len() == token.len();
    for (index, byte) in offered.iter().enumerate() {
        same &= token.get(index) == Some(byte);
    }
    same
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a test that cannot build a header is a broken test"
)]
mod tests {
    use super::{carries, from_this_machine};
    use axum::http::{HeaderMap, HeaderValue, header};

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(name, HeaderValue::from_str(value).expect("a header value"));
        }
        headers
    }

    #[test]
    fn a_request_from_a_page_is_refused_however_it_is_addressed() {
        assert!(!from_this_machine(&headers(&[
            (header::HOST, "127.0.0.1:41847"),
            (header::ORIGIN, "https://example.com"),
        ])));
        // Including a page served from this machine: a local development server
        // is still a browser, and a browser is what this check is about.
        assert!(!from_this_machine(&headers(&[
            (header::HOST, "127.0.0.1:41847"),
            (header::ORIGIN, "http://localhost:3000"),
        ])));
    }

    #[test]
    fn a_host_that_is_not_this_machine_is_the_rebinding_attack() {
        // The attacker's own name, resolved to the loopback. The socket sees a
        // connection from this machine; the `Host` is what gives it away.
        assert!(!from_this_machine(&headers(&[(
            header::HOST,
            "attacker.example:41847"
        )])));
        assert!(!from_this_machine(&HeaderMap::new()), "no Host, no trust");
    }

    #[test]
    fn a_client_naming_this_machine_gets_through() {
        for host in [
            "127.0.0.1:41847",
            "localhost:41847",
            "[::1]:41847",
            "127.0.0.1",
        ] {
            assert!(
                from_this_machine(&headers(&[(header::HOST, host)])),
                "`{host}` is this machine"
            );
        }
    }

    #[test]
    fn only_this_server_s_token_opens_the_door() {
        let token = "s3cret-token";
        assert!(carries(
            &headers(&[(header::AUTHORIZATION, "Bearer s3cret-token")]),
            token
        ));
        for offered in [
            "Bearer s3cret-toke",
            "Bearer s3cret-tokenn",
            "Bearer wrong",
            "s3cret-token",
            "Basic s3cret-token",
            "",
        ] {
            assert!(
                !carries(&headers(&[(header::AUTHORIZATION, offered)]), token),
                "`{offered}` is not this server's token"
            );
        }
        assert!(!carries(&HeaderMap::new(), token), "and nor is nothing");
    }
}
