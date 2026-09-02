//! One package's files, fetched from the computer that holds them.
//!
//! On a Mac an extension's code is a directory the application unpacked, and
//! `syncext://<id>/<path>` reads a file off the disk. A phone has no such
//! directory and will not be given one: what it has is a computer that does,
//! and the same URL answered over the channel.
//!
//! **This is not a second installation.** Installing a package writes two
//! things — the artefact on a machine, and the project's own declaration of it
//! together with the types it publishes. The second half is in the repository
//! and the phone already reads it; only the bytes were missing. So there is
//! nothing here to keep in step with the computer, nothing for a person to
//! manage, and no state in which the phone has a different version from the
//! machine that serves it.
//!
//! # What is kept, and why it is not an install either
//!
//! An answer is remembered under the whole URL. That is safe because of what
//! the URL carries: the computer builds it with the file's modification time in
//! `?v=`, so one URL names one immutable file and a rebuilt one is a different
//! URL. The window relies on the same thing — a webview memoises a module by
//! its specifier, so without the token nothing an author does would ever reach
//! the screen.
//!
//! In memory rather than on disk, and it ends with the process. What it buys is
//! the second and third read of one file during a session; what a disk cache
//! would add is a first paint before the computer answers, which is worth
//! nothing here — this window draws nothing at all without that computer.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine as _;
use serde_json::{Value, json};
use sync_memory::EXTENSION_FILE;
use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Manager as _, UriSchemeContext, UriSchemeResponder};

use crate::channel::Channel;

/// The scheme an extension's files are served under.
///
/// The same eight characters the computer builds its URLs with. Stated here
/// rather than imported because the other end is in the other application, and
/// what compares them is a URL that either resolves or does not.
pub const SCHEME: &str = "syncext";

/// The origins this window is served under.
///
/// The phone's webview is served from the bundle, so it is one of these two and
/// never a development server: there is no `devUrl` on a device. An origin that
/// is not one of them is somebody else asking, and there is nothing here for
/// them.
const WINDOW_ORIGINS: [&str; 2] = ["tauri://localhost", "http://tauri.localhost"];

/// Whether this request may read an artefact, and what to answer it with.
///
/// An absent Origin is not a cross-origin request and needs no permission; a
/// present one that is not this window's is somebody else asking. `Ok(None)` is
/// the first of those — permitted, with nothing to echo back.
///
/// Split out so it can be stated as a test rather than reasoned about. The
/// computer's own version of this rule was written once, exercised in one of
/// the two builds it had to hold in, and was false in the other — which is the
/// kind of thing a rule buried in a request handler does.
fn permitted(origin: &str) -> Result<Option<String>, &'static str> {
    if origin.is_empty() {
        return Ok(None);
    }
    if WINDOW_ORIGINS.contains(&origin) {
        return Ok(Some(origin.to_owned()));
    }
    Err("that origin may not read artefacts")
}

/// What has already been fetched this session.
#[derive(Default)]
pub struct Served {
    files: Mutex<HashMap<String, (String, Vec<u8>)>>,
}

/// Answer one request for a file of a package, off the reading thread.
///
/// A thread per request, because asking the computer blocks and the webview is
/// waiting on this: the alternative is a window that stops drawing while a
/// stylesheet is fetched. Tauri's asynchronous form exists for exactly this —
/// the responder outlives the call and is answered whenever the answer arrives.
pub fn serve<R: tauri::Runtime>(
    context: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = context.app_handle().clone();
    std::thread::spawn(move || responder.respond(answered(&app, &request)));
}

fn answered<R: tauri::Runtime>(
    app: &AppHandle<R>,
    request: &Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let origin = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    let allowed = match permitted(origin) {
        Ok(allowed) => allowed,
        Err(why) => return refused(StatusCode::FORBIDDEN, why),
    };

    let uri = request.uri();
    let Some(id) = uri.host().filter(|host| !host.is_empty()) else {
        return refused(StatusCode::BAD_REQUEST, "no extension named in the URI");
    };

    let held = uri.to_string();
    if let Some((media, bytes)) = remembered(app, &held) {
        return sent(&media, bytes, allowed);
    }

    // The path is sent as the computer's own door reads it, and the checks that
    // go with it stay there: whether anything serves that id, and whether the
    // path leaves the artefact. A phone cannot answer either question, and one
    // answered on both sides is one that will eventually be answered
    // differently.
    let asked = app
        .state::<Channel>()
        .ask(EXTENSION_FILE, &json!({"id": id, "path": uri.path()}));
    let answer = match asked {
        Ok(answer) => answer,
        Err(refusal) => return refused(StatusCode::NOT_FOUND, &refusal.to_string()),
    };

    let Some((media, bytes)) = read(&answer) else {
        return refused(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the computer answered with something that is not a file",
        );
    };
    keep(app, held, &media, &bytes);
    sent(&media, bytes, allowed)
}

/// The file the computer described, decoded.
fn read(answer: &Value) -> Option<(String, Vec<u8>)> {
    let media = answer.get("mediaType").and_then(Value::as_str)?;
    let bytes = answer.get("base64").and_then(Value::as_str)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(bytes)
        .ok()?;
    Some((media.to_owned(), bytes))
}

fn remembered<R: tauri::Runtime>(app: &AppHandle<R>, url: &str) -> Option<(String, Vec<u8>)> {
    app.try_state::<Served>()?
        .files
        .lock()
        .ok()?
        .get(url)
        .cloned()
}

fn keep<R: tauri::Runtime>(app: &AppHandle<R>, url: String, media: &str, bytes: &[u8]) {
    if let Some(served) = app.try_state::<Served>()
        && let Ok(mut files) = served.files.lock()
    {
        files.insert(url, (media.to_owned(), bytes.to_vec()));
    }
}

fn sent(media: &str, bytes: Vec<u8>, allowed: Option<String>) -> Response<Vec<u8>> {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", media)
        // Nothing is left to the webview's own store, for the reason the
        // computer gives: a URL names one immutable file, so what is worth
        // keeping is kept above, where it can be reasoned about.
        .header("cache-control", "no-store");
    if let Some(origin) = allowed {
        response = response.header("access-control-allow-origin", origin);
    }
    response
        .body(bytes)
        .unwrap_or_else(|_| refused(StatusCode::INTERNAL_SERVER_ERROR, "unreadable"))
}

/// A refusal the webview will show rather than a body it cannot read.
fn refused(status: StatusCode, why: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(why.as_bytes().to_vec())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use serde_json::json;

    use super::{permitted, read};

    /// The window may read artefacts, and nothing else may.
    #[test]
    fn only_this_window_reads_an_artefact() {
        assert_eq!(
            permitted("tauri://localhost"),
            Ok(Some("tauri://localhost".to_owned()))
        );
        assert_eq!(
            permitted("http://tauri.localhost"),
            Ok(Some("http://tauri.localhost".to_owned()))
        );
        // Not a cross-origin request at all, so there is nothing to permit and
        // nothing to echo back.
        assert_eq!(permitted(""), Ok(None));
        assert!(permitted("https://example.test").is_err());
    }

    /// A file arrives as what it is and as its bytes, and both are needed.
    #[test]
    fn a_file_is_read_back_as_its_bytes_and_its_type() {
        let (media, bytes) =
            read(&json!({"mediaType": "text/javascript", "base64": "ZXhwb3J0IHt9"}))
                .expect("the computer described a file");
        assert_eq!(media, "text/javascript");
        assert_eq!(bytes, b"export {}");
    }

    /// An answer that is not a file is not half-read into one.
    ///
    /// The webview is waiting on this, and a body built out of a missing media
    /// type would be a script served as whatever the guess was — which fails
    /// later, somewhere else, as a module that would not parse.
    #[test]
    fn an_answer_that_is_not_a_file_is_refused_whole() {
        assert!(read(&json!({"base64": "ZXhwb3J0IHt9"})).is_none());
        assert!(read(&json!({"mediaType": "text/javascript"})).is_none());
        assert!(read(&json!({"mediaType": "text/javascript", "base64": "not base64!"})).is_none());
    }
}
