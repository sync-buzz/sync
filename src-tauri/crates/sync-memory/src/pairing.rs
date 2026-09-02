//! The two things a device is handed, as one thing it can be handed.
//!
//! Pairing needs a machine's address and a key, and both are long strings of
//! hex that nobody is going to read out loud correctly. So they travel together
//! in one payload, which the window draws as a QR code and the device reads
//! with its camera.
//!
//! **Deliberately not a URL.** A QR that looks like a link is a QR the phone's
//! own camera offers to open, and what it would open is a key — into a browser,
//! into that browser's history, and past whatever else on the device claims the
//! scheme. Nothing registers this prefix, so a scanner that is not Sync shows
//! somebody a line of text and does nothing with it.
//!
//! The version sits inside the payload rather than being inferred from its
//! shape: a device that does not speak this one can say so, instead of reading
//! two fields out of something that meant three.

/// What every pairing payload starts with, version and all.
const PREFIX: &str = "sync-pair:1:";

/// What divides the address from the key. Neither may contain it, and neither
/// does — both are hex.
const BETWEEN: char = ':';

/// One payload carrying where to dial and what to say.
#[must_use]
pub fn pairing(endpoint: &str, secret: &str) -> String {
    format!("{PREFIX}{endpoint}{BETWEEN}{secret}")
}

/// Read one back, or nothing if this is not one of ours.
///
/// Refuses an empty half rather than answering with one. A device that dialled
/// an address with no key would get a refusal from the door and no idea which
/// of the two it had lost.
#[must_use]
pub fn paired(text: &str) -> Option<(String, String)> {
    let (endpoint, secret) = text.trim().strip_prefix(PREFIX)?.split_once(BETWEEN)?;
    if endpoint.is_empty() || secret.is_empty() || secret.contains(BETWEEN) {
        return None;
    }
    Some((endpoint.to_owned(), secret.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The round trip, which is the whole of what both ends have to agree on.
    #[test]
    fn what_one_end_writes_the_other_end_reads() {
        let payload = pairing("an-address", "a-key");
        assert_eq!(
            paired(&payload),
            Some(("an-address".to_owned(), "a-key".to_owned()))
        );
        // Whitespace a camera or a clipboard added on the way is not a
        // different payload.
        assert_eq!(paired(&format!("  {payload}\n")), paired(&payload));
    }

    /// Anything that is not one of ours is nothing, and it is worth listing the
    /// ones that are nearly right: those are what somebody scans by accident.
    #[test]
    fn something_that_is_not_a_pairing_payload_is_not_read_as_one() {
        for text in [
            "",
            "an-address:a-key",
            "sync-pair:an-address:a-key",
            "sync-pair:2:an-address:a-key",
            "sync-pair:1:an-address",
            "sync-pair:1::a-key",
            "sync-pair:1:an-address:",
            "https://example.test/sync-pair:1:a:b",
        ] {
            assert!(paired(text).is_none(), "`{text}` is not a payload");
        }
    }

    /// A key with a separator in it would split in the wrong place, so it is
    /// refused rather than read as a shorter key that would fail at the door.
    #[test]
    fn a_key_that_could_be_read_two_ways_is_read_no_ways() {
        assert!(paired("sync-pair:1:an-address:a:key").is_none());
    }
}
