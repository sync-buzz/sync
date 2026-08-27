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

use sync_extensions::{Net, net};

/// The host to read from, and it is the one the Issues package declares.
const DECLARED: &str = "api.github.com";

fn declaring(host: &str) -> Net {
    Net {
        hosts: vec![host.to_owned()],
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
    let answer = net::read(
        "issues",
        &format!("https://{DECLARED}/repos/rust-lang/rust/issues?state=open&per_page=1"),
        &declaring(DECLARED),
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
    assert!(
        answer.body.trim_start().starts_with('['),
        "the issues endpoint answers with a list"
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
    let refusal = net::read(
        "issues",
        &format!("https://{DECLARED}/repos/rust-lang/rust/issues"),
        &declaring("example.com"),
    )
    .expect_err("the package declared example.com and asked for GitHub")
    .to_string();

    assert!(refusal.contains(DECLARED), "{refusal}");
    assert!(refusal.contains("example.com"), "{refusal}");
    assert!(refusal.contains("issues"), "{refusal}");
}
