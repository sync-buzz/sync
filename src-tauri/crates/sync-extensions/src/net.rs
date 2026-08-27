//! What a package may reach, and the door it reaches it through.
//!
//! The second place this application dials out, and it is here for the same
//! reason [`crate::registry`] is: the window's `connect-src` names `'self'` and
//! the IPC endpoint and nothing else, so a package cannot fetch anything from
//! the webview and is not being restrained from doing so — there is no reach to
//! take away. What it gets instead is this, and every request goes through the
//! one function below with the package's own [`Net`] beside it.
//!
//! **The list belongs to the package, not to the build.** That is the whole
//! difference from the registry, where the hosts are compiled in: what one
//! extension may reach is a sentence in its own manifest, shown on the card
//! somebody installed it from. So the caller supplies both the URL and the
//! permission, and this refuses their disagreement.
//!
//! **Every hop is checked, not only the first**, exactly as the registry checks
//! them. A redirect to a host the package did not declare is the same request
//! reaching somewhere it may not, and an allow-list that admitted the first URL
//! and then followed wherever it was sent would be a list with a door in it.
//!
//! **It reads and cannot write.** There is no method, no body and no header on
//! the way out: a `GET`, and what the package may say about the request is
//! which URL. A header is where a token goes and a body is where an instruction
//! goes, and neither belongs to a package that was installed to read something.
//! When there is a reason for either, it arrives as a decision rather than as a
//! field that was already there.

use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::manifest::Net;

/// Nothing a package reads is worth holding a column for longer than this.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The largest answer this will read into memory.
///
/// A ceiling rather than trust in `Content-Length`, for the reason the registry
/// has one: the header is the server's claim and the memory is ours. Two
/// megabytes is far above any page of an API's listing and far below anything
/// that costs a person their window.
const LARGEST_ANSWER: u64 = 2 * 1024 * 1024;

/// The most hops before a redirect is a loop by another name.
const MOST_HOPS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("\"{0}\" is not a URL")]
    Unreadable(String),
    #[error("\"{0}\" is not https: a package reads over a connection nobody can read with it")]
    Plain(String),
    #[error(
        "\"{host}\" is not a host {id} declared: it reaches {declared}, and a request anywhere else is refused before it leaves this machine"
    )]
    Undeclared {
        id: String,
        host: String,
        declared: String,
    },
    #[error("{0} was not reached: {1}")]
    Unreachable(String, String),
    #[error("the answer is larger than a package may read")]
    TooLarge,
    #[error("the answer is not text")]
    NotText(String),
}

/// What came back, as the package reads it.
///
/// The status and the body, and deliberately nothing else. A package that can
/// see the status can say *this repository has no issues* and *GitHub is asking
/// for a token* differently, which is the difference a person needs; headers
/// are where the rest of an HTTP conversation lives, and a package reading one
/// is a package the surface would then have to keep in step with a protocol.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    /// What the server said about the request, `200` and everything else alike.
    ///
    /// Not turned into an error here. A `404` is an answer to a question the
    /// package asked and it is the package's to explain — *that repository has
    /// no issues* reads nothing like *the network is down*, and a layer that
    /// made both a failure would have thrown the difference away.
    pub status: u16,
    pub body: String,
}

/// Reads one URL for one package, or says why it did not.
///
/// # Errors
///
/// When the URL does not parse, is not `https`, or names a host the package did
/// not declare; when a redirect leaves that list; when the request fails or the
/// answer is larger than [`LARGEST_ANSWER`] or is not text.
pub fn read(id: &str, url: &str, allowed: &Net) -> Result<Answer, NetError> {
    let asked = admit(id, url, allowed)?;

    // Cloned into the redirect policy because the policy outlives this call's
    // stack frame as far as the client is concerned, and it is asked the same
    // question about a URL nobody here has seen.
    let hops = allowed.clone();
    let who = id.to_owned();
    let client = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("Sync/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > MOST_HOPS {
                attempt.error("too many redirects")
            } else if admit(&who, attempt.url().as_str(), &hops).is_ok() {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| NetError::Unreachable(asked.to_string(), error.to_string()))?;

    let response = client
        .get(asked.clone())
        .send()
        .map_err(|error| NetError::Unreachable(asked.to_string(), error.to_string()))?;

    // A 3xx arriving here is the policy above having stopped rather than a
    // server having answered, so the refusal is named as ours: the hop it was
    // about to take is the one the package did not declare.
    if response.status().is_redirection()
        && let Some(next) = response
            .headers()
            .get("location")
            .and_then(|to| to.to_str().ok())
    {
        admit(id, next, allowed)?;
    }

    let status = response.status().as_u16();
    let mut body = Vec::new();
    response
        .take(LARGEST_ANSWER + 1)
        .read_to_end(&mut body)
        .map_err(|error| NetError::Unreachable(asked.to_string(), error.to_string()))?;
    if body.len() as u64 > LARGEST_ANSWER {
        return Err(NetError::TooLarge);
    }

    let body = String::from_utf8(body).map_err(|_| NetError::NotText(asked.to_string()))?;
    Ok(Answer { status, body })
}

/// Whether one URL is one this package may reach, parsed rather than split.
///
/// Every shape that makes a URL read as one place and resolve to another is the
/// parser's business: credentials are not the host, a default port is not part
/// of a name, and a `..` segment is resolved before anything looks at the path.
/// Fails closed — a URL that does not parse at all is refused as one.
fn admit(id: &str, url: &str, allowed: &Net) -> Result<Url, NetError> {
    let parsed = Url::parse(url).map_err(|_| NetError::Unreadable(url.to_owned()))?;

    // Not a preference. A package's permission is a host name, and over plain
    // HTTP the host that answers is whoever is between the two of them.
    if parsed.scheme() != "https" {
        return Err(NetError::Plain(url.to_owned()));
    }

    let host = parsed.host_str().unwrap_or_default();
    if !allowed.admits(host) {
        return Err(NetError::Undeclared {
            id: id.to_owned(),
            host: host.to_owned(),
            declared: if allowed.hosts.is_empty() {
                "nothing".to_owned()
            } else {
                allowed.hosts.join(", ")
            },
        });
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn only(host: &str) -> Net {
        Net {
            hosts: vec![host.to_owned()],
        }
    }

    #[test]
    fn a_declared_host_is_reachable_and_nothing_else_is() {
        let allowed = only("api.github.com");
        assert!(
            admit(
                "issues",
                "https://api.github.com/repos/a/b/issues",
                &allowed
            )
            .is_ok()
        );

        // The shapes a URL uses to look like one host and be another, and the
        // parent of a declared host, which is a different place entirely.
        for elsewhere in [
            "https://api.github.com.evil.example/repos",
            "https://evil.example/api.github.com",
            "https://api.github.com@evil.example/repos",
            "https://github.com/repos",
            "https://evil.example/",
        ] {
            assert!(
                admit("issues", elsewhere, &allowed).is_err(),
                "{elsewhere} was admitted"
            );
        }
    }

    #[test]
    fn a_port_and_a_case_do_not_make_another_host() {
        let allowed = only("api.github.com");
        assert!(admit("issues", "https://API.GitHub.com/x", &allowed).is_ok());
        assert!(admit("issues", "https://api.github.com:443/x", &allowed).is_ok());
    }

    #[test]
    fn plain_http_is_refused_even_where_the_host_is_declared() {
        let allowed = only("api.github.com");
        let error =
            admit("issues", "http://api.github.com/x", &allowed).expect_err("http is not https");
        assert!(error.to_string().contains("https"), "{error}");
    }

    /// The refusal a person reads has to name the package, the host and what
    /// the package actually declared. Without the last of those it says a
    /// request was refused and leaves them to go and find the manifest.
    #[test]
    fn the_refusal_names_who_asked_and_what_they_may_reach() {
        let said = admit("issues", "https://evil.example/x", &only("api.github.com"))
            .expect_err("undeclared")
            .to_string();
        assert!(said.contains("issues"), "{said}");
        assert!(said.contains("evil.example"), "{said}");
        assert!(said.contains("api.github.com"), "{said}");
    }

    #[test]
    fn a_package_that_declared_nothing_reaches_nothing() {
        let error = admit("issues", "https://api.github.com/x", &Net::default())
            .expect_err("nothing was declared");
        assert!(error.to_string().contains("nothing"), "{error}");
    }
}
