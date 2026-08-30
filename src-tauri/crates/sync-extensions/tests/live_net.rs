//! A package's own door out, over the network.
//!
//! Ignored by default, for the reason `live_registry.rs` is: `cargo test` is a
//! statement about this code, and this file is a statement about GitHub. It
//! earns its place because the unit tests beside `net.rs` prove what is
//! *refused* and can prove nothing about what is admitted — a rule that
//! refused everything would pass all of them.
//!
//! ```text
//! cargo test -p sync-extensions --test live_net -- --ignored --nocapture
//! ```
#![allow(clippy::expect_used)]

use sync_extensions::{Net, NetRequest, net};

/// The host to read from, and it is the one the Issues package declares.
const DECLARED: &str = "api.github.com";

fn declaring(host: &str) -> Net {
    Net {
        hosts: vec![host.to_owned()],
        ..Net::default()
    }
}

/// Reads a public repository's issues with the permission that names the host.
///
/// This is the half the unit tests cannot reach: that the request is actually
/// made, that GitHub answers a machine with no account, and that what comes
/// back is a body a package can parse. `rust-lang/rust` because it is public,
/// old, and certain to have issues in it — the assertion is on the shape of the
/// answer rather than on anything anybody wrote in it.
#[test]
#[ignore = "talks to GitHub"]
fn a_declared_host_answers() {
    let answer = net::fetch(
        "issues",
        &NetRequest {
            url: format!("https://{DECLARED}/repos/rust-lang/rust/issues?state=open&per_page=1"),
            ..NetRequest::default()
        },
        &declaring(DECLARED),
        &std::collections::BTreeMap::new(),
    )
    .expect("api.github.com is reachable");

    // 403 here is the hour's sixty unauthenticated requests being spent, which
    // is a fact about this machine rather than about this code — so it is named
    // rather than left to fail as a mismatched status.
    assert_ne!(
        answer.status, 403,
        "GitHub is rate-limiting this machine, so this test cannot say anything"
    );
    assert_eq!(answer.status, 200, "GitHub answered {}", answer.status);
    assert!(answer.ok, "a 200 is one of the successful ones");
    assert!(
        answer.body.trim_start().starts_with('['),
        "the issues endpoint answers with a list"
    );

    // The half of the response the unit tests cannot reach either: a real
    // server's headers, read back under lower-cased names. `content-type` is
    // the one every API sends, so its absence would mean the map is empty
    // rather than that GitHub is unusual.
    assert!(
        answer.headers.contains_key("content-type"),
        "a real answer carries headers: {:?}",
        answer.headers
    );
    assert!(
        answer.url.starts_with(&format!("https://{DECLARED}/")),
        "the answer says where it came from: {}",
        answer.url
    );
}

/// The same request, from a package that declared somewhere else.
///
/// The refusal is the whole feature, so it is measured against the live host
/// rather than only against a parser: nothing leaves this machine, and the
/// sentence names the package, the host and what it may actually reach.
#[test]
#[ignore = "talks to GitHub"]
fn an_undeclared_host_is_refused_before_anything_leaves() {
    let refusal = net::fetch(
        "issues",
        &NetRequest {
            url: format!("https://{DECLARED}/repos/rust-lang/rust/issues"),
            ..NetRequest::default()
        },
        &declaring("example.com"),
        &std::collections::BTreeMap::new(),
    )
    .expect_err("the package declared example.com and asked for GitHub")
    .to_string();

    assert!(refusal.contains(DECLARED), "{refusal}");
    assert!(refusal.contains("example.com"), "{refusal}");
    assert!(refusal.contains("issues"), "{refusal}");
}
