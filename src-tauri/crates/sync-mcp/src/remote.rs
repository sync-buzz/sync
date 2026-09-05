//! The door a device off this machine comes through.
//!
//! The third transport on the same dispatcher, and the first one that is not on
//! this machine at all. What it serves is [`crate::host::Host`], unchanged, and
//! what it reads it with is the framing in [`crate::socket`] — a second copy of
//! either would be a second place the channel's rules are decided.
//!
//! # Why this is not a port
//!
//! A person's machine is behind whatever their home or their café put it
//! behind, and a port would work exactly as far as the same Wi-Fi. This is
//! QUIC over `iroh`: the two ends find each other by public key, punch through
//! where they can and fall back to a relay where they cannot, so *remote* means
//! remote rather than *upstairs*.
//!
//! It settles the other half too, and that is why the pairing secret is allowed
//! to travel on it at all. The connection is encrypted and both ends are
//! authenticated by their keys before this module sees a byte, so there is no
//! plaintext link for anybody to read the secret off and no name to spoof. A
//! door on a bare socket would have needed a challenge and a response to say
//! that much, and would still have carried every record in the clear.
//!
//! # What the door knows, and what it does not
//!
//! It holds the secrets it will admit and nothing else: no list of devices, no
//! names, no history. Those are the application's, kept beside its own
//! configuration and in the keychain, and they arrive here as a set of strings
//! over `remote.devices` whenever the person pairs or revokes one. So the whole
//! of this process's knowledge of who may come in ends when it does — which is
//! what makes revoking a device an act with an effect rather than a file edit.

use std::io;
use std::sync::{Arc, RwLock};

use iroh::endpoint::presets;
use iroh::{Endpoint, SecretKey};
use serde_json::{Value, json};
use tokio::io::BufReader;

use sync_memory::{MAX_FRAME_BYTES, REMOTE_GREETING, REMOTE_HELLO, REMOTE_IDLE};

use crate::application::Application;
use crate::host::Host;
use crate::socket::{Frame, Frames, Naming, attend_read, refusal, write};
use crate::watching::Subscriptions;

/// What this door is called when two ends agree what they are speaking.
///
/// Versioned in the name, beside the channel's own number rather than instead
/// of it: this one decides whether a connection is established at all, and a
/// client speaking something else is refused by QUIC before either end has
/// written anything. The number inside the channel is what catches the subtler
/// case — the right protocol, an older shape.
pub const ALPN: &[u8] = b"sync/host/1";

/// The one sentence anybody who is not admitted hears.
///
/// Every refusal on this door is this, whichever check produced it: no
/// greeting, a greeting that is not JSON, the wrong method, no secret, an
/// unknown secret. A door that distinguished them would be a door that answers
/// *that is a real device name, wrong secret* — which is the answer somebody
/// working through a list is looking for.
const REFUSED: &str = "this machine does not admit this connection";

/// How long a refused caller is given to hear its refusal.
///
/// Long enough for a message already written to reach anybody who is still
/// there, short enough that somebody trying secrets one after another cannot
/// hold a task per attempt by never reading the answer.
const FAREWELL: std::time::Duration = std::time::Duration::from_secs(2);

/// Who may come in, and who this machine is when it goes looking.
///
/// Both halves are stated from outside rather than found here: the secrets by
/// the application over `remote.devices`, the identity by [`serve`] once the
/// endpoint is bound. Empty is the ordinary state — an installation nobody has
/// paired a device with admits nobody, and says so by admitting nobody rather
/// than by not having a door.
#[derive(Default)]
pub(crate) struct Devices {
    admitted: RwLock<Vec<Admitted>>,
    /// This machine's public name on the network, once there is a door. It is
    /// what a person types into their phone, so it is answered rather than
    /// derived twice — the application does not link `iroh` and could not
    /// compute it from the key it minted.
    endpoint: RwLock<Option<String>>,
}

impl Devices {
    /// Take the devices the application says it has paired.
    ///
    /// Replaced whole rather than added to. Pairing and revoking are the same
    /// call, so a revocation cannot be the one message that goes missing.
    ///
    /// What survives the replacement is when each was last seen, for the
    /// fingerprints that are still in the set. The application states this on
    /// every read of its own status, so a set that forgot the times would be a
    /// set that forgot them several times a minute.
    pub(crate) fn stated(&self, params: &Value) {
        let stated: Vec<Admitted> = params
            .get("devices")
            .and_then(Value::as_array)
            .map(|devices| devices.iter().filter_map(Admitted::read).collect())
            .unwrap_or_default();
        if let Ok(mut admitted) = self.admitted.write() {
            // Carried across by fingerprint rather than by position: the
            // application states the set in whatever order it holds it.
            let seen: Vec<(String, Option<u64>)> = admitted
                .iter()
                .map(|device| (device.fingerprint.clone(), device.last_seen))
                .collect();
            *admitted = stated
                .into_iter()
                .map(|mut device| {
                    device.last_seen = seen
                        .iter()
                        .find(|(fingerprint, _)| *fingerprint == device.fingerprint)
                        .and_then(|(_, last_seen)| *last_seen);
                    device
                })
                .collect();
        }
    }

    /// What the application is told back: who this machine is, and how many
    /// devices this process is now holding for it.
    ///
    /// The count is there to be compared with what the application believes it
    /// sent. The two disagreeing means an engine restarted under a window that
    /// did not notice, and that is a thing to be able to see.
    pub(crate) fn described(&self) -> Value {
        let devices: Vec<Value> = self.admitted.read().map_or_else(
            |_| Vec::new(),
            |admitted| {
                admitted
                    .iter()
                    .map(|device| {
                        json!({
                            "fingerprint": device.fingerprint,
                            "lastSeen": device.last_seen,
                        })
                    })
                    .collect()
            },
        );
        json!({
            "endpoint": self.endpoint.read().ok().and_then(|held| held.clone()),
            "devices": devices,
        })
    }

    /// One device this machine admits.
    ///
    /// The fingerprint is what the application calls it by and carries nothing
    /// of the secret — it is minted beside one, not derived from it — so it is
    /// the half that can travel back in an answer.
    fn identified(&self, endpoint: &str) {
        if let Ok(mut held) = self.endpoint.write() {
            *held = Some(endpoint.to_owned());
        }
    }

    /// Whether `offered` is one of the secrets this machine admits.
    ///
    /// Every candidate is compared to its end and none of them stops the loop.
    /// A comparison that returned at the first match would say how many entries
    /// were tried, and one that stopped at the first wrong byte would say how
    /// much of a secret was right — both are answers to a caller working
    /// through guesses, one request at a time.
    fn admits(&self, offered: &str) -> Option<String> {
        let admitted = self.admitted.read().ok()?;
        let mut found = None;
        for device in admitted.iter() {
            if same(offered, &device.secret) {
                found = Some(device.fingerprint.clone());
            }
        }
        found
    }

    /// Write down that this device was here.
    ///
    /// Held in memory and nowhere else on this side. The application asks for
    /// it whenever it shows the list and keeps what it is told, which is what
    /// makes the time survive this process being restarted — the engine is not
    /// the place a fact about somebody's devices should outlive their session.
    fn seen(&self, fingerprint: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        if let Ok(mut admitted) = self.admitted.write()
            && let Some(device) = admitted
                .iter_mut()
                .find(|device| device.fingerprint == fingerprint)
        {
            device.last_seen = Some(now);
        }
    }
}

/// One paired device, as this process holds it.
struct Admitted {
    /// What the application calls it. Carries nothing of the secret.
    fingerprint: String,
    secret: String,
    /// Seconds since the epoch, or nothing since this process started.
    last_seen: Option<u64>,
}

impl Admitted {
    fn read(stated: &Value) -> Option<Self> {
        Some(Self {
            fingerprint: stated.get("fingerprint")?.as_str()?.to_owned(),
            secret: stated.get("secret")?.as_str()?.to_owned(),
            last_seen: None,
        })
    }
}

/// Whether two secrets are the same, in time that does not depend on where
/// they first differ.
fn same(offered: &str, held: &str) -> bool {
    let offered = offered.as_bytes();
    let held = held.as_bytes();
    let mut same = offered.len() == held.len();
    for (index, byte) in offered.iter().enumerate() {
        same &= held.get(index) == Some(byte);
    }
    same
}

/// Serve the channel to paired devices until the process ends.
///
/// # Errors
///
/// When the endpoint cannot be bound — no network, or a machine that will not
/// give out a UDP socket. Reported rather than retried: the application is
/// watching this call and shows the person why their door did not open.
pub(crate) async fn serve(
    host: Arc<Host>,
    application: Arc<Application>,
    devices: Arc<Devices>,
    subscriptions: Arc<Subscriptions>,
    key: SecretKey,
) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = bound(key).await?;
    let identity = endpoint.id().to_string();
    devices.identified(&identity);
    tracing::info!(endpoint = %identity, "the network door is open");
    attending(host, application, devices, subscriptions, endpoint).await;
    Ok(())
}

/// Take this machine's place on the network, under the key it is known by.
///
/// # Errors
///
/// Whatever binding refused — no network, or a machine that will not give out
/// a UDP socket.
async fn bound(key: SecretKey) -> Result<Endpoint, Box<dyn std::error::Error>> {
    Endpoint::builder(presets::N0)
        .secret_key(key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .map_err(Into::into)
}

/// Answer devices until the endpoint is closed.
async fn attending(
    host: Arc<Host>,
    application: Arc<Application>,
    devices: Arc<Devices>,
    subscriptions: Arc<Subscriptions>,
    endpoint: Endpoint,
) {
    while let Some(incoming) = endpoint.accept().await {
        let host = Arc::clone(&host);
        let application = Arc::clone(&application);
        let devices = Arc::clone(&devices);
        let subscriptions = Arc::clone(&subscriptions);
        // Per connection, for the reason the socket spawns per connection: one
        // device's call must not be behind another's.
        tokio::spawn(async move {
            let connection = match incoming.await {
                Ok(connection) => connection,
                // Not worth a name: this is a connection that failed to
                // establish, which on a network is weather.
                Err(error) => {
                    tracing::debug!(%error, "a connection did not establish");
                    return;
                }
            };
            let who = connection.remote_id().to_string();
            if let Err(error) =
                admit(&host, &application, &devices, &subscriptions, connection).await
            {
                // Ended is ordinary — a phone put away, a network changed — so
                // the reason is what makes the line worth writing.
                tracing::info!(device = %who, %error, "a device's connection ended");
            }
        });
    }
}

/// Find out who this is, and let them at the channel if they are anybody.
async fn admit(
    host: &Arc<Host>,
    application: &Arc<Application>,
    devices: &Arc<Devices>,
    subscriptions: &Arc<Subscriptions>,
    connection: iroh::endpoint::Connection,
) -> io::Result<()> {
    let who = connection.remote_id().to_string();
    let (writing, reading) = connection
        .accept_bi()
        .await
        .map_err(|error| io::Error::other(format!("no stream on the connection: {error}")))?;

    // The same framing the window's own door reads, with the same ceiling on a
    // message and a deadline on top of it. The deadline is the greeting's
    // rather than the connection's: a caller that has not said who it is has
    // not earned the ten minutes an admitted one gets.
    let mut lines = Frames::new(BufReader::new(reading), MAX_FRAME_BYTES).patience(REMOTE_GREETING);
    let mut writing = writing;

    // Every way this can fail is the same refusal, said once, after which the
    // connection is closed rather than kept for a second attempt: no greeting
    // in time, a greeting too long, a stream that ended.
    let Ok(Frame::Line(greeting)) = lines.next().await else {
        return turn_away(&connection, &mut writing, &Value::Null, &who).await;
    };
    let Ok(said) = serde_json::from_str::<Value>(&greeting) else {
        return turn_away(&connection, &mut writing, &Value::Null, &who).await;
    };
    let id = said.get("id").cloned().unwrap_or(Value::Null);
    let admitted = if said.get("method").and_then(Value::as_str) == Some(REMOTE_HELLO) {
        said.get("params")
            .and_then(|params| params.get("secret"))
            .and_then(Value::as_str)
            .and_then(|secret| devices.admits(secret))
    } else {
        None
    };
    let Some(fingerprint) = admitted else {
        return turn_away(&connection, &mut writing, &id, &who).await;
    };
    devices.seen(&fingerprint);

    tracing::info!(device = %who, %fingerprint, "a device came in");
    write(
        &mut writing,
        &json!({"jsonrpc": "2.0", "id": id, "result": {"admitted": true}}),
    )
    .await?;

    // From here it is the window's channel exactly, minus the two things a
    // caller off this machine does not get: it names its projects by key, and
    // it cannot turn the connection around. Both are decided in `socket.rs`,
    // because both are properties of the door rather than of the caller.
    attend_read(
        host,
        application,
        devices,
        subscriptions,
        lines.patience(REMOTE_IDLE),
        writing,
        &Naming { by_path: false },
    )
    .await
}

/// Say the one sentence, and only then go.
///
/// The order is the point, and it has to be made to happen: closing a QUIC
/// connection discards whatever was still on its way, so a refusal written on
/// the line before the connection is dropped is a refusal nobody ever reads.
/// What the caller would see instead is the connection vanishing — which is the
/// one answer this door must not give, because it is indistinguishable from a
/// machine that is off.
async fn turn_away(
    connection: &iroh::endpoint::Connection,
    writing: &mut iroh::endpoint::SendStream,
    id: &Value,
    who: &str,
) -> io::Result<()> {
    tracing::warn!(device = %who, "a connection was refused");
    // Best effort: a caller that has already gone is an ordinary way for this
    // to end, and the refusal is for the one that is still there.
    let _ = write(writing, &refusal(id, REFUSED)).await;
    let _ = writing.finish();
    // Bounded, because the caller that will not acknowledge this is exactly the
    // caller that was just refused, and it does not get to keep a task.
    let _ = tokio::time::timeout(FAREWELL, connection.closed()).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    use tokio::io::AsyncBufReadExt;

    use crate::projects::{Projects, Registered};

    /// The whole door, over a real connection between two endpoints.
    ///
    /// `#[ignore]`d for the reason the tests that reach GitHub are: it binds a
    /// UDP socket and talks to `iroh`'s discovery, so it is a test about the
    /// outside world and a green run without it says nothing about this.
    /// Asked for by name:
    ///
    /// ```sh
    /// cargo test -p sync-mcp --bin sync-mcp -- --ignored a_paired_device
    /// ```
    ///
    /// What it settles is the claim the rest of this module rests on — that
    /// what answers over the network is the surface itself, not a copy of it.
    /// `projects.list` is asked because the answer comes from the registry this
    /// test handed the surface, so an answer that agrees could not have come
    /// from anywhere else.
    #[tokio::test]
    #[ignore = "binds a UDP socket and reaches iroh's discovery"]
    async fn a_paired_device_reaches_the_surface_and_an_unpaired_one_reaches_nothing() {
        let registered = vec![Registered {
            path: std::path::PathBuf::from("/w/a"),
            name: "A".to_owned(),
            identifier: "A".to_owned(),
        }];
        let host = Arc::new(Host::over(Arc::new(Projects::over(registered, None)), None));
        let devices = Arc::new(holding(&["a-paired-phone"]).take());
        let door = bound(SecretKey::generate()).await.expect("the door bound");
        let dialling = door.addr();
        tokio::spawn(attending(
            host,
            Arc::new(Application::new()),
            Arc::clone(&devices),
            Arc::new(Subscriptions::default()),
            door,
        ));

        let phone = Endpoint::builder(presets::N0)
            .bind()
            .await
            .expect("the device bound");

        // The device this machine knows: admitted, and then answered by the
        // surface itself.
        let mut talking = dialled(&phone, dialling.clone(), "a-paired-phone").await;
        assert_eq!(talking.0["result"]["admitted"], true);
        let listed = talking
            .1
            .ask(json!({
                "jsonrpc": "2.0", "id": 2, "method": sync_memory::PROJECTS, "params": {},
            }))
            .await;
        assert_eq!(listed["result"]["projects"][0]["project"], "A");

        // And one it does not: the same sentence whichever check produced it,
        // and nothing after it.
        let turned = dialled(&phone, dialling, "a-secret-nobody-minted").await;
        assert_eq!(turned.0["error"]["message"], REFUSED);
        assert!(turned.0["result"].is_null());
    }

    /// One connection to the door, held open with what it takes to keep asking.
    struct Talking {
        writing: iroh::endpoint::SendStream,
        reading: tokio::io::Lines<tokio::io::BufReader<iroh::endpoint::RecvStream>>,
        // Kept because dropping a connection closes its streams.
        _connection: iroh::endpoint::Connection,
    }

    impl Talking {
        async fn ask(&mut self, request: Value) -> Value {
            self.writing
                .write_all(format!("{request}\n").as_bytes())
                .await
                .expect("the device wrote");
            self.answer().await
        }

        async fn answer(&mut self) -> Value {
            let line = self
                .reading
                .next_line()
                .await
                .expect("the answer is readable")
                .expect("an answer arrived");
            serde_json::from_str(&line).expect("the answer is JSON")
        }
    }

    /// Dial the door, say the secret, and hand back what it said with the
    /// connection it said it on.
    async fn dialled(
        phone: &Endpoint,
        dialling: iroh::EndpointAddr,
        secret: &str,
    ) -> (Value, Talking) {
        let connection = phone.connect(dialling, ALPN).await.expect("it connected");
        let (writing, reading) = connection.open_bi().await.expect("a stream");
        let mut talking = Talking {
            writing,
            reading: tokio::io::BufReader::new(reading).lines(),
            _connection: connection,
        };
        let greeted = talking
            .ask(json!({
                "jsonrpc": "2.0", "id": 1, "method": REMOTE_HELLO,
                "params": {"secret": secret},
            }))
            .await;
        (greeted, talking)
    }

    /// A machine admitting these secrets, each under a fingerprint made of its
    /// position — enough to tell them apart, and never derived from the secret.
    fn holding(secrets: &[&str]) -> Held {
        let devices = Devices::default();
        devices.stated(&json!({
            "devices": secrets
                .iter()
                .enumerate()
                .map(|(at, secret)| json!({"fingerprint": format!("f{at}"), "secret": secret}))
                .collect::<Vec<_>>(),
        }));
        Held(devices)
    }

    /// A set of devices a test can either read or hand over.
    struct Held(Devices);

    impl Held {
        fn take(self) -> Devices {
            self.0
        }
    }

    impl std::ops::Deref for Held {
        type Target = Devices;

        fn deref(&self) -> &Devices {
            &self.0
        }
    }

    /// The whole of what the door decides, and it decides it from a set the
    /// application stated rather than from anything on this disk.
    #[test]
    fn only_a_stated_secret_is_admitted() {
        let devices = holding(&["a-paired-phone", "a-paired-tablet"]);
        assert_eq!(devices.admits("a-paired-phone").as_deref(), Some("f0"));
        assert_eq!(devices.admits("a-paired-tablet").as_deref(), Some("f1"));
        for offered in ["a-paired-phon", "a-paired-phonee", "", "A-PAIRED-PHONE"] {
            assert!(
                devices.admits(offered).is_none(),
                "`{offered}` is not a paired device"
            );
        }
    }

    /// Revoking is stating the set without that device in it, and it takes
    /// effect on the next connection rather than on a restart.
    #[test]
    fn a_revoked_device_stops_being_admitted() {
        let devices = holding(&["a-paired-phone", "a-lost-phone"]);
        assert!(devices.admits("a-lost-phone").is_some());
        devices.stated(&json!({
            "devices": [{"fingerprint": "f0", "secret": "a-paired-phone"}],
        }));
        assert!(devices.admits("a-lost-phone").is_none());
        assert!(devices.admits("a-paired-phone").is_some());
    }

    /// An installation nobody has paired anything with admits nobody — said
    /// here because "empty set" and "no checking" are one typo apart.
    #[test]
    fn a_machine_with_no_paired_device_admits_nothing() {
        let devices = Devices::default();
        assert!(devices.admits("").is_none());
        assert!(devices.admits("anything at all").is_none());
        devices.stated(&json!({}));
        assert!(devices.admits("anything at all").is_none());
    }

    /// The count is the application's way of telling this process apart from
    /// one that restarted under it, so it has to be the count of what was
    /// actually taken.
    #[test]
    fn the_answer_states_what_is_held_and_when_each_was_here() {
        let devices = holding(&["one", "two"]);
        let described = devices.described();
        assert_eq!(described["devices"][0]["fingerprint"], "f0");
        // Nothing has connected, and that is `null` rather than a time: a
        // device that has never been here and one that was here at the epoch
        // must not read alike.
        assert!(described["devices"][0]["lastSeen"].is_null());
        // No door open yet, so there is no name to give either.
        assert!(described["endpoint"].is_null());
        devices.identified("abcdef");
        assert_eq!(devices.described()["endpoint"], "abcdef");
    }

    /// The time a device was here survives the application stating the set
    /// again, which it does every time somebody looks at the list.
    #[test]
    fn a_device_that_was_here_keeps_its_time_across_a_restatement() {
        let devices = holding(&["one", "two"]);
        devices.seen("f1");
        let was = devices.described()["devices"][1]["lastSeen"].clone();
        assert!(was.is_u64(), "it was here: {was}");

        devices.stated(&json!({
            "devices": [
                {"fingerprint": "f0", "secret": "one"},
                {"fingerprint": "f1", "secret": "two"},
            ],
        }));
        assert_eq!(devices.described()["devices"][1]["lastSeen"], was);
        // And a device that has gone takes its time with it.
        devices.stated(&json!({"devices": [{"fingerprint": "f0", "secret": "one"}]}));
        assert_eq!(
            devices.described()["devices"].as_array().map(Vec::len),
            Some(1)
        );
    }
}
