//! Reading the ChatGPT account id out of an access token.
//!
//! Codex requests must name the account the subscription belongs to. The
//! provider carries that id as a claim inside the access token rather than as
//! a separate response field, so it is read back out after each exchange.

use base64::Engine as _;

/// Claim namespace that holds OpenAI's auth metadata.
const AUTH_CLAIM: &str = "https://api.openai.com/auth";

/// Extracts the ChatGPT account id from a JWT access token.
///
/// Returns `None` when the token is not a JWT, its payload is unreadable, or
/// the account claim is absent or empty.
#[must_use]
pub fn account_id_from_access_token(access_token: &str) -> Option<String> {
    let payload = jwt_payload(access_token)?;
    let account_id = payload
        .get(AUTH_CLAIM)?
        .get("chatgpt_account_id")?
        .as_str()?;
    (!account_id.is_empty()).then(|| account_id.to_owned())
}

/// Decodes the payload segment of a JWT without verifying its signature.
///
/// The token is a bearer credential jinn just received over TLS from the
/// issuer; the payload is read for the account id it carries, not trusted as
/// an authorization decision.
fn jwt_payload(token: &str) -> Option<serde_json::Value> {
    let mut segments = token.split('.');
    let _header = segments.next()?;
    let payload = segments.next()?;
    let _signature = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    fn token_with_payload(payload: &serde_json::Value) -> String {
        let encode = |value: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value);
        format!(
            "{}.{}.{}",
            encode(b"{\"alg\":\"none\"}"),
            encode(
                serde_json::to_string(payload)
                    .expect("serialize")
                    .as_bytes()
            ),
            encode(b"signature")
        )
    }

    #[rstest::rstest]
    fn account_id_is_read_from_the_auth_claim() {
        // Given an access token carrying a ChatGPT account id.
        let token = token_with_payload(&serde_json::json!({
            AUTH_CLAIM: { "chatgpt_account_id": "acct-123" }
        }));

        // When extracting the account id.
        let account_id = account_id_from_access_token(&token);

        // Then the claim's value is returned.
        assert_eq!(account_id.as_deref(), Some("acct-123"));
    }

    #[rstest::rstest]
    fn a_token_without_the_claim_yields_nothing() {
        // Given an access token with no auth claim.
        let token = token_with_payload(&serde_json::json!({ "sub": "user-1" }));

        // When extracting the account id.
        // Then nothing is returned.
        assert!(account_id_from_access_token(&token).is_none());
    }

    #[rstest::rstest]
    fn an_opaque_token_yields_nothing() {
        // Given a token that is not a JWT.
        // When extracting the account id.
        // Then nothing is returned rather than panicking.
        assert!(account_id_from_access_token("not-a-jwt").is_none());
    }

    #[rstest::rstest]
    fn an_empty_account_id_yields_nothing() {
        // Given an access token whose account id claim is empty.
        let token = token_with_payload(&serde_json::json!({
            AUTH_CLAIM: { "chatgpt_account_id": "" }
        }));

        // When extracting the account id.
        // Then the empty value is rejected.
        assert!(account_id_from_access_token(&token).is_none());
    }
}
