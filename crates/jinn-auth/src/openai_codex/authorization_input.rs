//! Parsing whatever the user pastes back from the authorization page.
//!
//! Browsers and providers hand the result back in several shapes. Rather than
//! demanding one, jinn accepts the full redirect URL, a bare
//! `code=...&state=...` query fragment, the `code#state` form Codex shows, or
//! the authorization code on its own.

/// An authorization result pasted by the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthorizationInput {
    /// The authorization code, when one could be found.
    pub code: Option<String>,
    /// The `state` value that accompanied the code, when present.
    pub state: Option<String>,
}

/// Parses pasted authorization input.
#[must_use]
pub fn parse(input: &str) -> AuthorizationInput {
    let value = input.trim();
    if value.is_empty() {
        return AuthorizationInput::default();
    }

    if let Ok(url) = url::Url::parse(value) {
        let mut parsed = AuthorizationInput::default();
        for (key, item) in url.query_pairs() {
            match key.as_ref() {
                "code" => parsed.code = Some(item.into_owned()),
                "state" => parsed.state = Some(item.into_owned()),
                _ => {}
            }
        }
        return parsed;
    }

    if let Some((code, state)) = value.split_once('#') {
        return AuthorizationInput {
            code: Some(code.to_owned()),
            state: Some(state.to_owned()),
        };
    }

    if value.contains("code=") {
        let mut parsed = AuthorizationInput::default();
        for (key, item) in url::form_urlencoded::parse(value.as_bytes()) {
            match key.as_ref() {
                "code" => parsed.code = Some(item.into_owned()),
                "state" => parsed.state = Some(item.into_owned()),
                _ => {}
            }
        }
        return parsed;
    }

    AuthorizationInput {
        code: Some(value.to_owned()),
        state: None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn a_full_redirect_url_yields_its_code_and_state() {
        // Given the whole redirect URL copied from the browser.
        let input = "http://localhost:1455/auth/callback?code=abc123&state=xyz";

        // When parsing it.
        let parsed = parse(input);

        // Then both the code and the state are recovered.
        assert_eq!(
            parsed,
            AuthorizationInput {
                code: Some("abc123".to_owned()),
                state: Some("xyz".to_owned()),
            }
        );
    }

    #[rstest::rstest]
    fn a_bare_query_string_yields_its_code_and_state() {
        // Given only the query portion of the redirect.
        let input = "code=abc123&state=xyz";

        // When parsing it.
        let parsed = parse(input);

        // Then both the code and the state are recovered.
        assert_eq!(
            parsed,
            AuthorizationInput {
                code: Some("abc123".to_owned()),
                state: Some("xyz".to_owned()),
            }
        );
    }

    #[rstest::rstest]
    fn the_hash_separated_form_yields_its_code_and_state() {
        // Given the `code#state` form the authorization page displays.
        let input = "abc123#xyz";

        // When parsing it.
        let parsed = parse(input);

        // Then both the code and the state are recovered.
        assert_eq!(
            parsed,
            AuthorizationInput {
                code: Some("abc123".to_owned()),
                state: Some("xyz".to_owned()),
            }
        );
    }

    #[rstest::rstest]
    fn a_bare_code_is_taken_as_the_code() {
        // Given just the authorization code.
        let input = "  abc123  ";

        // When parsing it.
        let parsed = parse(input);

        // Then it is taken as the code, with no state to check.
        assert_eq!(
            parsed,
            AuthorizationInput {
                code: Some("abc123".to_owned()),
                state: None,
            }
        );
    }

    #[rstest::rstest]
    fn empty_input_yields_nothing() {
        // Given blank input.
        // When parsing it.
        let parsed = parse("   ");

        // Then no code is produced.
        assert_eq!(parsed, AuthorizationInput::default());
    }
}
