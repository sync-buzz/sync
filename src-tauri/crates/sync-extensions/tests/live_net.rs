//! A package's own door out, over the network.
//!
//! Ignored by default, for the reason `live_registry.rs` is: `cargo test` is a
//! statement about this code, and this file is a statement about two servers
//! nobody here runs. It earns its place because the unit tests beside `net.rs`
//! prove what is *refused* and can prove nothing about what is admitted — a
//! rule that refused everything would pass all of them, and so would a door
//! that sent every picture as the base64 it was written as.
//!
//! ```text
//! cargo test -p sync-extensions --test live_net -- --ignored --nocapture
//! ```
#![allow(clippy::expect_used)]

use sync_extensions::{Net, NetMethod, NetPart, NetRequest, net};

/// The host to read from, and it is the one the Issues package declares.
const DECLARED: &str = "api.github.com";

/// A host that says back what it was sent.
///
/// Reading proves itself against any public API; sending proves nothing unless
/// something repeats the request, and no API worth reading will describe the
/// bytes it was handed. So the half of this door that writes is measured
/// against a service whose whole purpose is to echo.
const ECHO: &str = "httpbin.org";

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

/// A picture leaves as bytes and a form arrives with its parts intact.
///
/// **The one thing no unit test beside `net.rs` can say.** Those weigh what is
/// sent and refuse what cannot be, and every one of them would still pass if
/// the base64 went out as the string it was written as, or if the boundary in
/// the header named something the body does not contain. What settles it is a
/// server reading the request back: the four bytes of a PNG's signature come
/// back as four bytes, the field beside the file comes back as a field, and the
/// name on disk survives the trip.
#[test]
#[ignore = "talks to an echo service"]
fn a_form_arrives_as_the_parts_it_was_given() {
    // The first four bytes of every PNG, and the point of using them is that
    // they are not text: a door that sent the base64 on would echo back the
    // letters `iVBORw`, and a door that mangled the bytes would echo neither.
    let signature = [0x89_u8, b'P', b'N', b'G'];
    let answer = net::fetch(
        "uploads",
        &NetRequest {
            url: format!("https://{ECHO}/post"),
            method: NetMethod::Post,
            form: Some(vec![
                NetPart {
                    name: "title".to_owned(),
                    text: Some("the login screen".to_owned()),
                    ..NetPart::default()
                },
                NetPart {
                    name: "photo".to_owned(),
                    base64: Some(base64_of(&signature)),
                    filename: Some("screenshot.png".to_owned()),
                    content_type: Some("image/png".to_owned()),
                    ..NetPart::default()
                },
            ]),
            ..NetRequest::default()
        },
        &declaring(ECHO),
        &std::collections::BTreeMap::new(),
    )
    .expect("the echo service is reachable");

    assert_eq!(answer.status, 200, "it answered {}", answer.status);
    let said: serde_json::Value =
        serde_json::from_str(&answer.body).expect("the echo answers with JSON");

    assert_eq!(
        said["form"]["title"], "the login screen",
        "the field beside the file: {}",
        answer.body
    );
    // Echoed as a `data:` URL, which is how this service writes bytes it cannot
    // put in JSON — so the assertion is on what follows the comma.
    let photo = said["files"]["photo"]
        .as_str()
        .expect("the file came back as a string");
    assert!(
        photo.ends_with(&base64_of(&signature)),
        "the bytes made the trip whole: {photo}"
    );
    assert!(
        said["headers"]["Content-Type"]
            .as_str()
            .unwrap_or_default()
            .starts_with("multipart/form-data; boundary="),
        "the boundary is written where the body is assembled: {}",
        said["headers"]
    );
}

/// The same bytes with no form around them, which is what a presigned upload is.
#[test]
#[ignore = "talks to an echo service"]
fn bytes_with_no_form_around_them_arrive_as_bytes() {
    let signature = [0x89_u8, b'P', b'N', b'G'];
    let answer = net::fetch(
        "uploads",
        &NetRequest {
            url: format!("https://{ECHO}/put"),
            method: NetMethod::Put,
            headers: std::collections::BTreeMap::from([(
                "content-type".to_owned(),
                "image/png".to_owned(),
            )]),
            body_base64: Some(base64_of(&signature)),
            ..NetRequest::default()
        },
        &declaring(ECHO),
        &std::collections::BTreeMap::new(),
    )
    .expect("the echo service is reachable");

    assert_eq!(answer.status, 200, "it answered {}", answer.status);
    let said: serde_json::Value =
        serde_json::from_str(&answer.body).expect("the echo answers with JSON");
    let sent = said["data"].as_str().expect("it says what it was sent");

    assert!(
        sent.ends_with(&base64_of(&signature)),
        "the body was the bytes rather than their spelling: {sent}"
    );
    assert_eq!(
        said["headers"]["Content-Type"], "image/png",
        "a request with no form says what its own body is"
    );
}

/// The encoding as a package writes it, so the test states it once.
fn base64_of(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
