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
//! **A request carries a method, headers and a body, and what changes something
//! is behind a second capability.** An earlier version of this door had none of
//! the three: a `GET`, and the only thing a package could say about a request
//! was which URL. That was the right shape while the only reason to dial out
//! was to read, and it stopped being right the first time a package had to
//! answer somewhere — an agent finishes and the extension writes the result
//! into somebody else's tracker, which no `GET` can do.
//!
//! What replaced the restriction is a person being told. `net` is reading, and
//! [`crate::manifest::NET_WRITE_CAPABILITY`] is changing something at the other
//! end, because those are two things to agree to and a card that said only the
//! first would be describing the smaller of them.
//!
//! **What a package sends is text, bytes or a form, and it says which.** One
//! member cannot mean all three, and the one that was here first means text: a
//! picture put in it would have been sent as the string of base64 it was
//! written as, which is not what any server was asked for. So they are spelled
//! apart and at most one is given — `body` is text, `bodyBase64` is bytes,
//! encoded only because a process boundary carries no bytes, and `form` is
//! `multipart/form-data`.
//!
//! **The form is here because it cannot be composed above this line.** Its
//! boundary is written into a header and repeated between every part, so
//! whatever builds the body has to write the header too; a package doing that
//! for itself would be naming a boundary in a request whose body this door
//! assembled. Nothing else about multipart needs to be here, and nothing else
//! is: what a part is called, what it is called on disk and what it contains
//! are the package's to say.
//!
//! **The verb is what is gated, and headers are not.** A header carries a token
//! or a content type, and the argument that it is a body by another name proves
//! too much: a package composes the URL, so it could put anything it wanted in
//! a query string and did not need a header to do it. What a verb changes is
//! not what leaves this machine but what the other end is being asked to do,
//! and that is the thing worth a person's agreement.
//!
//! **Nothing here retries.** A request that timed out may have been performed,
//! and whether to send it again is a question only the package can answer.

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use reqwest::blocking::multipart;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::manifest::{Net, Secret};

/// Nothing a package reads is worth holding a column for longer than this.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The largest response this will read into memory.
///
/// A ceiling rather than trust in `Content-Length`, for the reason the registry
/// has one: the header is the server's claim and the memory is ours. Two
/// megabytes is far above any page of an API's listing and far below anything
/// that costs a person their window.
const LARGEST_RESPONSE: u64 = 2 * 1024 * 1024;

/// The most a package may push out of this window in one request.
///
/// It was [`LARGEST_RESPONSE`] again, and the symmetry was the whole argument:
/// what a package reads and what it sends are the same size of thing. A picture
/// is where that stopped being true. A photograph off a phone is three to five
/// megabytes, and a door that read a page of an API's listing without complaint
/// and refused every camera would be a door with an accident in it, so the two
/// numbers part company here and this one says why.
///
/// **Measured against what is actually sent.** Bytes arrive as base64 because a
/// process boundary carries no bytes, and base64 is a third larger than what it
/// encodes — a ceiling read off the string would be a ceiling a quarter below
/// where it says it is. So it is the decoded length that is weighed, and for a
/// form it is every part added up.
const LARGEST_BODY: usize = 8 * 1024 * 1024;

/// Headers the transport owns, which a package may not write.
///
/// Not a security boundary — the URL decides which machine is connected to, and
/// none of these change that. It is a refusal in place of a puzzle: a package
/// that sets its own `content-length` has written a request that disagrees with
/// itself, and the failure would arrive from a server as something unrelated.
///
/// The list itself is the manifest's, so that what is refused when a file is
/// read and what is refused when a request is made cannot come apart.
use crate::manifest::THE_TRANSPORT_S;

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
    #[error("the response is larger than a package may read")]
    TooLarge,
    #[error("the response is not text")]
    NotText(String),
    #[error(
        "a {0} carries no body: what a package sends with one is what the other end is asked to store, and this method asks it to store nothing"
    )]
    BodyWithoutAVerb(Method),
    #[error("the body is larger than a package may send")]
    TooMuchToSend,
    #[error(
        "a request says once what it sends, and this one says it {0} times: `body` is text, `bodyBase64` is bytes and `form` is a multipart form, and one request carries one of the three"
    )]
    MoreThanOneBody(usize),
    #[error(
        "{0} was given as base64 and does not read as base64: what belongs there is the encoding on its own, and a `data:` prefix left on the front of it is the usual reason this is read"
    )]
    NotBase64(String),
    #[error(
        "a form with no parts in it sends nothing: a request with nothing to put in one carries no form"
    )]
    FormWithoutParts,
    #[error(
        "a part of a form has no name: every part is looked up at the other end by the name it was sent under, and one with none arrives somewhere nothing reads"
    )]
    PartWithoutAName,
    #[error(
        "the \"{0}\" part carries neither `text` nor `base64`: it is a name the other end is given with nothing under it"
    )]
    PartWithoutContent(String),
    #[error(
        "the \"{0}\" part carries both `text` and `base64`, and one part is one thing: say which of them is being sent"
    )]
    PartWithTwoContents(String),
    #[error(
        "\"{0}\" is the form's to write: multipart carries a boundary that is generated where the body is assembled, and a package naming its own would be describing a request it did not send"
    )]
    TheFormS(String),
    #[error("\"{0}\" is not usable as the type of the \"{1}\" part: {2}")]
    UnusableType(String, String, String),
    #[error(
        "\"{0}\" is the transport's to write: a request that sets its own would disagree with itself, and the server would answer about something else"
    )]
    TheTransportS(String),
    #[error("\"{0}\" is not usable as a header: {1}")]
    UnusableHeader(String, String),
    #[error(
        "\"{0}\" is a header {1} declares a secret for, so the value in it is not the package's to write: name the secret in the manifest, or write a different header"
    )]
    AlreadySpokenFor(String, String),
}

/// What a request asks the other end to do.
///
/// An enum rather than a string, so a method nothing here can honour is refused
/// where the request is read rather than sent and puzzled over. The spelling is
/// the protocol's — uppercase — because that is what an author writing against
/// somebody else's API is reading in their documentation.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    #[default]
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    /// Whether this asks the other end to change something.
    ///
    /// The division this whole door's second capability rests on, and it is the
    /// protocol's own: `GET` and `HEAD` are defined as safe, and everything
    /// else is defined as being allowed to have an effect. A method that is
    /// merely *usually* harmless is not a category anybody can agree to.
    #[must_use]
    pub fn changes_something(self) -> bool {
        !matches!(self, Self::Get | Self::Head)
    }

    /// Whether a body means anything with this method.
    #[must_use]
    fn carries_a_body(self) -> bool {
        self.changes_something()
    }

    fn spelling(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, into: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        into.write_str(self.spelling())
    }
}

/// One request, as the package states it.
///
/// **Unknown members are refused rather than dropped.** The shape is `fetch`'s
/// vocabulary narrowed to what crosses a process boundary, so an author will
/// reach for the parts of `fetch` that are not here — `signal`, `credentials`,
/// `redirect`. A member silently ignored is a timeout somebody believes they
/// set; a member refused by name is a sentence they can act on.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub url: String,
    /// `GET` when the package does not say, which is what `fetch` does.
    #[serde(default)]
    pub method: Method,
    /// Header names and values, as the package writes them.
    ///
    /// A map rather than a list of pairs: a package sending one name twice is
    /// saying something confused, and the map makes that unsayable rather than
    /// leaving this door to decide which of the two it meant.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// What is sent, when it is text.
    #[serde(default)]
    pub body: Option<String>,
    /// What is sent, when it is bytes: a picture, a signature, an archive.
    ///
    /// Base64 because the value crosses a process boundary as JSON and JSON has
    /// no bytes. The encoding is undone here and the request carries what was
    /// encoded — a package that leaves a `data:` prefix on the front of it is
    /// refused rather than sending a body beginning with the word `data`.
    #[serde(default)]
    pub body_base64: Option<String>,
    /// What is sent, when the other end asked for a form.
    ///
    /// The shape almost every *upload this file* API is written against, and
    /// the one thing on this door a package could not have composed for itself:
    /// see the note on the module.
    #[serde(default)]
    pub form: Option<Vec<Part>>,
}

/// One part of a form, as the package states it.
///
/// **A part is one thing.** `text` or `base64`, never both and never neither: a
/// part carrying two values is a package that has not decided what it is
/// sending, and one carrying none is a name the other end is handed with
/// nothing under it.
///
/// `filename` is what makes a part a file rather than a field, and it is the
/// package's to choose — most APIs keep it, some show it to a person later, and
/// nothing here is in a position to know which. `contentType` is the same: what
/// the bytes are is known where they came from and nowhere after.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Part {
    /// What the other end looks this part up by.
    pub name: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub base64: Option<String>,
    /// The name the part carries as a file, when it is one.
    #[serde(default)]
    pub filename: Option<String>,
    /// What is in it — `image/png` — for a server that does not guess.
    #[serde(default)]
    pub content_type: Option<String>,
}

/// What came back, as the package reads it.
///
/// `fetch`'s vocabulary, and every member of it is one a package cannot do its
/// work without. `status` and `ok` are the same fact at two useful widths —
/// almost every caller wants the second and the one that does not needs the
/// first exactly. `headers` are where pagination and rate limits live, and a
/// package polling somebody else's tracker without `link` or `retry-after` is
/// one that either stops at the first page or gets itself blocked.
///
/// No `statusText`: HTTP/2 does not carry a reason phrase at all, so it would
/// be a member that is sometimes there — which is worse than one that never is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    /// Where the response actually came from, after any redirect.
    ///
    /// Not the URL that was asked for, and the difference is the point: a
    /// package that followed a redirect and then builds its next request from
    /// the address it started at will keep being redirected.
    pub url: String,
    /// What the server said about the request, `200` and everything else alike.
    ///
    /// Not turned into an error here. A `404` is an answer to a question the
    /// package asked and it is the package's to explain — *that repository has
    /// no issues* reads nothing like *the network is down*, and a layer that
    /// made both a failure would have thrown the difference away.
    pub status: u16,
    /// Whether the status is one of the successful ones, as `fetch` derives it.
    ///
    /// Derived here rather than in whichever surface is asking, so that the
    /// window and a service module read the same fact rather than two
    /// implementations of it.
    pub ok: bool,
    /// The response's headers, names in lower case.
    ///
    /// A name repeated by the server arrives as one entry with its values
    /// joined by `, `, which is how the protocol says a list-valued header may
    /// be written in the first place.
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// Makes one request for one package, or says why it did not.
///
/// **The one place a request is built.** Whatever asks — the window's command
/// layer, and anything else that is handed a package's permission — states what
/// it wants as a [`Request`] and this turns it into a request that leaves the
/// machine. A second builder somewhere else would be a second set of rules
/// about what may leave it.
///
/// Whether the package may use a method that changes something is *not* decided
/// here: this is handed a request and a permission, and who may ask for which
/// is settled where the application knows who is calling. [`Method::changes_something`]
/// is what that decision is made on, so both halves read one definition of it.
///
/// # Errors
///
/// When the URL does not parse, is not `https`, or names a host the package did
/// not declare; when a redirect leaves that list; when the request carries a
/// body it cannot carry, a body too large to send or a header the transport
/// owns; when the request fails, or the response is larger than
/// [`LARGEST_RESPONSE`] or is not text.
pub fn fetch(
    id: &str,
    request: &Request,
    allowed: &Net,
    sealed: &BTreeMap<String, String>,
) -> Result<Response, NetError> {
    let asked = admit(id, &request.url, allowed)?;
    let sent = sent(request)?;

    // The form writes `content-type`, so nothing else may — including a
    // manifest, which is the case a package cannot fix from its own code. It is
    // named as the form's rather than as the transport's because it is only the
    // form's: a package sending text says what its text is, and always could.
    if matches!(sent, Some(Sending::Form(_))) && sealed.contains_key("content-type") {
        return Err(NetError::TheFormS("content-type".to_owned()));
    }

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

    let mut sending = client.request(verb(request.method), asked.clone());
    for (name, value) in &request.headers {
        let lowered = name.to_ascii_lowercase();
        if THE_TRANSPORT_S.contains(&lowered.as_str()) {
            return Err(NetError::TheTransportS(name.clone()));
        }
        if lowered == "content-type" && matches!(sent, Some(Sending::Form(_))) {
            return Err(NetError::TheFormS(name.clone()));
        }
        // **A declared secret is not overwritten and not silently ignored.**
        // Either behaviour would be the same header meaning two things: one
        // sends a token the author thought they had replaced, the other drops a
        // value the manifest promised a person would be sent. So the request is
        // refused and says which of the two to change.
        if sealed.contains_key(&lowered) {
            return Err(NetError::AlreadySpokenFor(name.clone(), id.to_owned()));
        }
        sending = sending.header(
            header_name(name)?,
            reqwest::header::HeaderValue::try_from(value.as_str())
                .map_err(|error| NetError::UnusableHeader(name.clone(), error.to_string()))?,
        );
    }
    // Last, so that nothing above can have written them, and marked sensitive
    // so that a value never reaches a log through a formatter that thought it
    // was being helpful.
    for (name, value) in sealed {
        let mut value = reqwest::header::HeaderValue::try_from(value.as_str())
            .map_err(|error| NetError::UnusableHeader(name.clone(), error.to_string()))?;
        value.set_sensitive(true);
        sending = sending.header(header_name(name)?, value);
    }
    if let Some(sent) = sent {
        sending = attach(sending, sent)?;
    }

    let response = sending
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
    let ok = response.status().is_success();
    let url = response.url().to_string();
    let headers = read_headers(response.headers());
    let mut body = Vec::new();
    response
        .take(LARGEST_RESPONSE + 1)
        .read_to_end(&mut body)
        .map_err(|error| NetError::Unreachable(asked.to_string(), error.to_string()))?;
    if body.len() as u64 > LARGEST_RESPONSE {
        return Err(NetError::TooLarge);
    }

    let body = String::from_utf8(body).map_err(|_| NetError::NotText(asked.to_string()))?;
    Ok(Response {
        url,
        status,
        ok,
        headers,
        body,
    })
}

/// The request with what it sends on it.
///
/// The last thing done to it, so that nothing above can have written a header
/// this settles: the form's `content-type` is written here, out of a boundary
/// that does not exist until the parts are assembled.
///
/// # Errors
///
/// When a part names a type that is not a media type.
fn attach(
    sending: reqwest::blocking::RequestBuilder,
    sent: Sending,
) -> Result<reqwest::blocking::RequestBuilder, NetError> {
    Ok(match sent {
        Sending::One(Payload::Text(text)) => sending.body(text),
        Sending::One(Payload::Bytes(bytes)) => sending.body(bytes),
        Sending::Form(parts) => {
            let mut form = multipart::Form::new();
            for part in parts {
                let named = part.name.clone();
                let mut piece = match part.content {
                    Payload::Text(text) => multipart::Part::text(text),
                    Payload::Bytes(bytes) => multipart::Part::bytes(bytes),
                };
                if let Some(filename) = part.filename {
                    piece = piece.file_name(filename);
                }
                if let Some(kind) = part.content_type {
                    piece = piece.mime_str(&kind).map_err(|error| {
                        NetError::UnusableType(kind.clone(), named.clone(), error.to_string())
                    })?;
                }
                form = form.part(named, piece);
            }
            sending.multipart(form)
        }
    })
}

/// What one request puts on the wire, once the package's three spellings of
/// *what I am sending* have been resolved to the one thing that is sent.
///
/// Here rather than as three branches inside [`fetch`] because the size a
/// request may be, the verb it needs and the decoding of it are the same three
/// questions whichever spelling was used, and asking them once is what stops
/// the answers drifting apart.
#[derive(Debug)]
enum Sending {
    One(Payload),
    Form(Vec<ReadyPart>),
}

/// Text or bytes, after base64 has been undone.
#[derive(Debug)]
enum Payload {
    Text(String),
    Bytes(Vec<u8>),
}

impl Payload {
    /// What it weighs on the wire, which is not what it weighed as base64.
    fn size(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Bytes(bytes) => bytes.len(),
        }
    }
}

/// One part of a form with its content decoded and its name settled.
#[derive(Debug)]
struct ReadyPart {
    name: String,
    content: Payload,
    filename: Option<String>,
    content_type: Option<String>,
}

/// What the request sends, or why it sends nothing.
///
/// `None` for a request with no body at all, which is most of them.
///
/// # Errors
///
/// When more than one of the three is given, when base64 is not base64, when a
/// part of a form is not one thing, when the method carries no body, or when
/// what is sent is larger than [`LARGEST_BODY`].
fn sent(request: &Request) -> Result<Option<Sending>, NetError> {
    let said = usize::from(request.body.is_some())
        + usize::from(request.body_base64.is_some())
        + usize::from(request.form.is_some());
    if said > 1 {
        return Err(NetError::MoreThanOneBody(said));
    }

    let sending = match (&request.body, &request.body_base64, &request.form) {
        (Some(text), _, _) => Sending::One(Payload::Text(text.clone())),
        (_, Some(encoded), _) => Sending::One(Payload::Bytes(decoded("`bodyBase64`", encoded)?)),
        (_, _, Some(parts)) => Sending::Form(ready(parts)?),
        _ => return Ok(None),
    };

    if !request.method.carries_a_body() {
        return Err(NetError::BodyWithoutAVerb(request.method));
    }
    let size = match &sending {
        Sending::One(payload) => payload.size(),
        // The parts, and not the envelope around them: the boundaries and the
        // names are this door's own writing, and a package cannot be refused
        // for the length of something it did not compose.
        Sending::Form(parts) => parts.iter().map(|part| part.content.size()).sum(),
    };
    if size > LARGEST_BODY {
        return Err(NetError::TooMuchToSend);
    }

    Ok(Some(sending))
}

/// The bytes behind a package's base64, or a refusal naming where it was.
///
/// The refusal says which value it was reading, because a form has several and
/// *that is not base64* about an unnamed one is a sentence an author cannot act
/// on.
fn decoded(what: &str, encoded: &str) -> Result<Vec<u8>, NetError> {
    BASE64
        .decode(encoded.as_bytes())
        .map_err(|_| NetError::NotBase64(what.to_owned()))
}

/// A form's parts with their content decoded, or the first refusal among them.
fn ready(parts: &[Part]) -> Result<Vec<ReadyPart>, NetError> {
    if parts.is_empty() {
        return Err(NetError::FormWithoutParts);
    }
    parts
        .iter()
        .map(|part| {
            let name = part.name.trim();
            if name.is_empty() {
                return Err(NetError::PartWithoutAName);
            }
            let content = match (&part.text, &part.base64) {
                (Some(_), Some(_)) => return Err(NetError::PartWithTwoContents(name.to_owned())),
                (Some(text), None) => Payload::Text(text.clone()),
                (None, Some(encoded)) => {
                    Payload::Bytes(decoded(&format!("the \"{name}\" part"), encoded)?)
                }
                (None, None) => return Err(NetError::PartWithoutContent(name.to_owned())),
            };
            Ok(ReadyPart {
                name: name.to_owned(),
                content,
                filename: part.filename.clone(),
                content_type: part.content_type.clone(),
            })
        })
        .collect()
}

/// A header name the client will accept, or a refusal naming it.
fn header_name(name: &str) -> Result<reqwest::header::HeaderName, NetError> {
    reqwest::header::HeaderName::try_from(name)
        .map_err(|error| NetError::UnusableHeader(name.to_owned(), error.to_string()))
}

/// Which of a package's declared secrets belong to one request.
///
/// Matched on the host of the URL, which is the same question [`admit`] asks
/// and is asked here separately for one reason: reading a value is the
/// application's — the keychain has exactly one door and this crate is not it —
/// so what this crate can answer is *which*, and the answer is carried back in
/// as [`fetch`]'s `sealed`.
///
/// Answers nothing for a URL that does not parse. The request is refused a
/// moment later for the same reason, and a secret looked up for a request that
/// will not be made is a keychain prompt somebody did not need to see.
#[must_use]
pub fn secrets_for<'a>(url: &str, allowed: &'a Net) -> Vec<&'a Secret> {
    let Ok(parsed) = Url::parse(url) else {
        return Vec::new();
    };
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    allowed
        .secrets
        .iter()
        .filter(|sending| sending.host.eq_ignore_ascii_case(&host))
        .collect()
}

/// The method, as the client spells it.
fn verb(method: Method) -> reqwest::Method {
    match method {
        Method::Get => reqwest::Method::GET,
        Method::Head => reqwest::Method::HEAD,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
    }
}

/// What came back, flattened to one value per name.
///
/// A header the server sent twice is joined with `, `, which is the spelling
/// the protocol already allows for a list-valued header — so a package parsing
/// one gets the same string whichever way the server chose to write it. A value
/// that is not text is dropped rather than lossily rewritten: a package cannot
/// act on bytes it cannot read, and a mangled string is worse than an absence
/// it can test for.
fn read_headers(headers: &reqwest::header::HeaderMap) -> BTreeMap<String, String> {
    let mut read: BTreeMap<String, String> = BTreeMap::new();
    for (name, value) in headers {
        let Ok(value) = value.to_str() else { continue };
        read.entry(name.as_str().to_ascii_lowercase())
            .and_modify(|already| {
                already.push_str(", ");
                already.push_str(value);
            })
            .or_insert_with(|| value.to_owned());
    }
    read
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
            ..Net::default()
        }
    }

    fn asking(method: Method) -> Request {
        Request {
            url: "https://api.github.com/repos/a/b/issues".to_owned(),
            method,
            ..Request::default()
        }
    }

    /// The division the second capability rests on, taken from the protocol
    /// rather than from a guess about which verbs are harmless.
    #[test]
    fn the_safe_methods_are_the_ones_that_change_nothing() {
        assert!(!Method::Get.changes_something());
        assert!(!Method::Head.changes_something());
        for verb in [Method::Post, Method::Put, Method::Patch, Method::Delete] {
            assert!(verb.changes_something(), "{verb} changes something");
        }
    }

    /// The default is `GET`, as it is in the shape this borrows its vocabulary
    /// from: a package that says only where to go is reading.
    #[test]
    fn a_request_that_names_no_method_is_a_read() {
        let request: Request =
            serde_json::from_str(r#"{"url": "https://api.github.com/x"}"#).expect("it reads");
        assert_eq!(request.method, Method::Get);
        assert!(request.body.is_none());
        assert!(request.headers.is_empty());
    }

    /// **The member an author will reach for that is not here.** `signal`,
    /// `credentials`, `redirect` — every one of them is something `fetch` has
    /// and this does not, and dropping one silently would be a package
    /// believing it had set a timeout.
    #[test]
    fn a_member_this_door_does_not_have_is_refused_rather_than_dropped() {
        let refused = serde_json::from_str::<Request>(
            r#"{"url": "https://api.github.com/x", "signal": null}"#,
        )
        .expect_err("a member that is not here is refused");
        assert!(
            refused.to_string().contains("signal"),
            "the refusal names the member: {refused}"
        );
    }

    /// A body is what the other end is asked to store, and a method that asks
    /// it to store nothing has nowhere to put one. Refused before anything is
    /// sent, because a server's answer to this would be about something else.
    #[test]
    fn a_body_on_a_method_that_carries_none_is_refused() {
        let spellings: [fn(&mut Request); 3] = [
            |request| request.body = Some("{}".to_owned()),
            |request| request.body_base64 = Some(BASE64.encode("{}")),
            |request| {
                request.form = Some(vec![Part {
                    name: "photo".to_owned(),
                    base64: Some(BASE64.encode("{}")),
                    ..Part::default()
                }]);
            },
        ];

        for say in spellings {
            let mut request = asking(Method::Get);
            say(&mut request);

            let refused = fetch(
                "issues",
                &request,
                &only("api.github.com"),
                &BTreeMap::new(),
            )
            .expect_err("a GET carries no body");

            assert!(
                matches!(refused, NetError::BodyWithoutAVerb(Method::Get)),
                "{refused}"
            );
        }
    }

    /// The ceiling on text, which is the same one the bytes are weighed against.
    #[test]
    fn a_body_larger_than_a_package_may_send_is_refused() {
        let mut request = asking(Method::Post);
        request.body = Some("x".repeat(LARGEST_BODY + 1));

        let refused = fetch(
            "issues",
            &request,
            &only("api.github.com"),
            &BTreeMap::new(),
        )
        .expect_err("more than a package may send");

        assert!(matches!(refused, NetError::TooMuchToSend), "{refused}");
    }

    /// **Bytes cross as base64 and leave as bytes.** The encoding is only how a
    /// picture gets through a boundary that carries JSON; a door that sent it
    /// on would be sending the spelling rather than the thing spelled, and
    /// every server would answer about a body of ASCII it was not offered.
    #[test]
    fn bytes_are_decoded_before_they_are_sent() {
        let mut request = asking(Method::Post);
        request.body_base64 = Some(BASE64.encode([0x89, b'P', b'N', b'G']));

        match sent(&request).expect("it reads") {
            Some(Sending::One(Payload::Bytes(bytes))) => {
                assert_eq!(bytes, vec![0x89, b'P', b'N', b'G']);
            }
            _ => panic!("bytes were given and bytes are sent"),
        }
    }

    /// The mistake this refusal is for. A `data:` URL is what a browser hands
    /// somebody who copies an image, and the encoding is only the tail of it —
    /// pasted whole it decodes to nothing, and the answer arrives from a server
    /// as something about a malformed upload.
    #[test]
    fn base64_that_is_a_whole_data_url_is_refused_by_name() {
        let mut request = asking(Method::Post);
        request.body_base64 = Some("data:image/png;base64,iVBORw0KGgo=".to_owned());

        let refused = sent(&request).expect_err("the prefix is not the encoding");

        assert!(
            matches!(&refused, NetError::NotBase64(what) if what.contains("bodyBase64")),
            "{refused}"
        );
        assert!(
            refused.to_string().contains("data:"),
            "the refusal names the usual cause: {refused}"
        );
    }

    /// One request sends one thing. Two members given is a package that has not
    /// decided, and picking one for it would send whichever this door happened
    /// to test for first.
    #[test]
    fn a_request_says_once_what_it_sends() {
        let mut request = asking(Method::Post);
        request.body = Some("{}".to_owned());
        request.body_base64 = Some(BASE64.encode("x"));

        let refused = sent(&request).expect_err("two spellings of one member");

        assert!(matches!(refused, NetError::MoreThanOneBody(2)), "{refused}");
    }

    /// **The ceiling is what leaves the machine, not what was written down.**
    /// Base64 is a third larger than what it encodes, so a limit read off the
    /// string would sit a quarter below where it says it does — and the number
    /// a person was told is the one about their photograph.
    #[test]
    fn what_is_weighed_is_the_bytes_and_not_the_base64_that_carried_them() {
        let mut request = asking(Method::Post);
        request.body_base64 = Some(BASE64.encode(vec![0u8; 7 * 1024 * 1024]));
        assert!(
            request.body_base64.as_ref().map_or(0, String::len) > LARGEST_BODY,
            "the encoding is over the ceiling that the bytes are under"
        );
        assert!(sent(&request).is_ok(), "seven megabytes of picture is sent");

        request.body_base64 = Some(BASE64.encode(vec![0u8; LARGEST_BODY + 1]));
        assert!(
            matches!(sent(&request), Err(NetError::TooMuchToSend)),
            "and one byte past the ceiling is not"
        );
    }

    /// A part is one thing, named. Every other shape is a package that has not
    /// said what it is uploading, and the refusal says which part it was.
    #[test]
    fn a_part_of_a_form_is_one_named_thing() {
        let named = |part: Part| {
            let mut request = asking(Method::Post);
            request.form = Some(vec![part]);
            sent(&request).expect_err("a part this door cannot send")
        };

        assert!(matches!(
            named(Part {
                name: "photo".to_owned(),
                text: Some("hello".to_owned()),
                base64: Some(BASE64.encode("hello")),
                ..Part::default()
            }),
            NetError::PartWithTwoContents(which) if which == "photo"
        ));
        assert!(matches!(
            named(Part {
                name: "photo".to_owned(),
                ..Part::default()
            }),
            NetError::PartWithoutContent(which) if which == "photo"
        ));
        assert!(matches!(
            named(Part {
                name: "   ".to_owned(),
                text: Some("hello".to_owned()),
                ..Part::default()
            }),
            NetError::PartWithoutAName
        ));

        let mut empty = asking(Method::Post);
        empty.form = Some(Vec::new());
        assert!(matches!(
            sent(&empty).expect_err("a form with nothing in it"),
            NetError::FormWithoutParts
        ));
    }

    /// A form is several things with their own names and types, and what it
    /// weighs is what is in it rather than the envelope this door writes round
    /// them.
    #[test]
    fn a_form_carries_its_parts_with_what_each_of_them_is() {
        let mut request = asking(Method::Post);
        request.form = Some(vec![
            Part {
                name: "title".to_owned(),
                text: Some("the login screen".to_owned()),
                ..Part::default()
            },
            Part {
                name: "photo".to_owned(),
                base64: Some(BASE64.encode([0x89, b'P', b'N', b'G'])),
                filename: Some("screenshot.png".to_owned()),
                content_type: Some("image/png".to_owned()),
                ..Part::default()
            },
        ]);

        match sent(&request).expect("it reads") {
            Some(Sending::Form(parts)) => {
                assert_eq!(parts.len(), 2);
                assert_eq!(parts[1].filename.as_deref(), Some("screenshot.png"));
                assert_eq!(parts[1].content_type.as_deref(), Some("image/png"));
                assert_eq!(parts[1].content.size(), 4, "the bytes, not their base64");
            }
            _ => panic!("a form was given and a form is sent"),
        }
    }

    /// **The boundary is written where the body is assembled, so the header
    /// naming it is too.** A package that wrote its own — or a manifest that
    /// sent one as a secret, which the package could not have overridden — would
    /// describe a request that was never sent, and the server would answer about
    /// a body it could not find.
    #[test]
    fn the_form_writes_the_content_type_and_nothing_else_may() {
        let photo = || {
            Some(vec![Part {
                name: "photo".to_owned(),
                base64: Some(BASE64.encode([0x89, b'P', b'N', b'G'])),
                ..Part::default()
            }])
        };

        let mut request = asking(Method::Post);
        request.form = photo();
        request
            .headers
            .insert("Content-Type".to_owned(), "image/png".to_owned());

        let refused = fetch(
            "issues",
            &request,
            &only("api.github.com"),
            &BTreeMap::new(),
        )
        .expect_err("the form writes that one");
        assert!(
            matches!(&refused, NetError::TheFormS(name) if name == "Content-Type"),
            "{refused}"
        );

        let mut declared = asking(Method::Post);
        declared.form = photo();
        let sealed = BTreeMap::from([("content-type".to_owned(), "image/png".to_owned())]);

        let refused = fetch("issues", &declared, &only("api.github.com"), &sealed)
            .expect_err("not even one a manifest declared");
        assert!(
            matches!(&refused, NetError::TheFormS(name) if name == "content-type"),
            "{refused}"
        );
    }

    /// A header the transport writes for itself is refused by name, so that a
    /// request that would have disagreed with itself never leaves.
    #[test]
    fn a_header_the_transport_owns_is_refused_by_name() {
        for owned in ["content-length", "Host", "Connection", "transfer-encoding"] {
            let mut request = asking(Method::Post);
            request.body = Some("{}".to_owned());
            request
                .headers
                .insert(owned.to_owned(), "whatever".to_owned());

            let refused = fetch(
                "issues",
                &request,
                &only("api.github.com"),
                &BTreeMap::new(),
            )
            .expect_err("the transport writes this one");

            assert!(
                matches!(&refused, NetError::TheTransportS(name) if name == owned),
                "{refused}"
            );
        }
    }

    /// **The header a manifest declared is not the package's to write.** Not
    /// overwritten and not ignored: one of those sends a token the author
    /// thought they had replaced, the other drops a value the card promised a
    /// person would be sent. Both are the same header meaning two things, so
    /// the request is refused and the refusal says which of them to change.
    #[test]
    fn a_header_a_declared_secret_owns_cannot_be_written_by_the_package() {
        let mut request = asking(Method::Get);
        request
            .headers
            .insert("Authorization".to_owned(), "Bearer mine".to_owned());
        let sealed = BTreeMap::from([("authorization".to_owned(), "Bearer theirs".to_owned())]);

        let refused = fetch("issues", &request, &only("api.github.com"), &sealed)
            .expect_err("the manifest already spoke for that header");

        assert!(
            matches!(&refused, NetError::AlreadySpokenFor(name, id)
                if name == "Authorization" && id == "issues"),
            "{refused}"
        );
        assert!(
            !refused.to_string().contains("theirs"),
            "a refusal about a secret does not carry one: {refused}"
        );
    }

    /// Which pairs belong to a request is decided by the host, and by nothing
    /// else: a package that reaches two APIs sends each its own.
    #[test]
    fn a_secret_is_matched_to_the_host_it_was_declared_for() {
        let allowed = Net {
            hosts: vec!["api.github.com".to_owned(), "api.other.example".to_owned()],
            secrets: vec![Secret {
                host: "api.github.com".to_owned(),
                header: "authorization".to_owned(),
                secret: "token".to_owned(),
                scheme: Some("Bearer".to_owned()),
            }],
        };

        assert_eq!(
            secrets_for("https://API.GitHub.com/repos", &allowed).len(),
            1,
            "a host has no case, and neither does the match"
        );
        assert!(secrets_for("https://api.other.example/x", &allowed).is_empty());
        assert!(
            secrets_for("not a url at all", &allowed).is_empty(),
            "a request that will be refused reads nobody's keychain first"
        );
    }

    /// And a host outside the list is refused before any of that is looked at,
    /// whatever the method is: writing somewhere undeclared is the thing this
    /// door exists to stop, and it is stopped in the same place reading is.
    #[test]
    fn a_write_to_an_undeclared_host_is_refused_like_a_read() {
        for method in [Method::Get, Method::Post, Method::Delete] {
            let request = Request {
                url: "https://evil.example/x".to_owned(),
                method,
                ..Request::default()
            };

            let refused = fetch(
                "issues",
                &request,
                &only("api.github.com"),
                &BTreeMap::new(),
            )
            .expect_err("nowhere but the declared host");

            assert!(
                matches!(refused, NetError::Undeclared { .. }),
                "a {method} to an undeclared host: {refused}"
            );
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
