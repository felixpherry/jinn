//! PKCE verifier and challenge generation (RFC 7636, `S256`).

use base64::Engine as _;
use rand::Rng as _;
use sha2::{Digest as _, Sha256};

/// A PKCE verifier and its derived challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    /// The high-entropy secret kept by the client until the token exchange.
    pub verifier: String,
    /// The `S256` hash of the verifier, sent with the authorization request.
    pub challenge: String,
}

/// Number of random bytes behind a verifier. RFC 7636 allows 32-96 octets.
const VERIFIER_BYTES: usize = 32;

/// Generates a fresh verifier/challenge pair.
#[must_use]
pub fn generate() -> Pkce {
    let verifier_bytes: [u8; VERIFIER_BYTES] = rand::rng().random();
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = challenge_for(&verifier);
    Pkce {
        verifier,
        challenge,
    }
}

/// Derives the `S256` challenge for a verifier.
#[must_use]
pub fn challenge_for(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Generates an opaque random `state` value for CSRF protection.
#[must_use]
pub fn random_state() -> String {
    use std::fmt::Write as _;
    let bytes: [u8; 16] = rand::rng().random();
    bytes
        .iter()
        .fold(String::with_capacity(32), |mut state, byte| {
            let _ = write!(state, "{byte:02x}");
            state
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn challenge_matches_the_known_s256_vector() {
        // Given the verifier from RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

        // When deriving its challenge.
        let challenge = challenge_for(verifier);

        // Then it equals the published challenge.
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[rstest::rstest]
    fn generated_challenge_is_derived_from_its_verifier() {
        // Given a generated PKCE pair.
        let pkce = generate();

        // When re-deriving the challenge from the verifier.
        // Then it matches the pair's challenge.
        assert_eq!(challenge_for(&pkce.verifier), pkce.challenge);
    }

    #[rstest::rstest]
    fn two_generated_verifiers_differ() {
        // Given two generated PKCE pairs.
        let first = generate();
        let second = generate();

        // When comparing their verifiers.
        // Then they are not the same secret.
        assert_ne!(first.verifier, second.verifier);
    }

    #[rstest::rstest]
    fn generated_state_values_differ() {
        // Given two generated state values.
        // When comparing them.
        // Then they are not the same.
        assert_ne!(random_state(), random_state());
    }
}
