//! `SessionId`: which `permark mcp` process a request comes from. Drawn once per process and sent with its hello and
//! every call, so the app tells two agents on one project apart and can name the agent behind each request. Not a
//! secret: processes of the same user share the OS boundary that guards the endpoint.

use permark_core::projects::uid::{draw_token, MAX_TOKEN_CHARS};
use serde::{Deserialize, Serialize};

/// Shortest | longest id accepted from the wire.
const MIN_LEN: usize = 8;
const MAX_LEN: usize = 32;

/// 8..32 chars of `[a-z0-9]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// An id as sent: `None` when it is not 8..32 chars of `[a-z0-9]`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let ok = (MIN_LEN..=MAX_LEN).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        ok.then(|| Self(text.to_owned()))
    }

    /// A fresh id for this process.
    #[must_use]
    pub fn draw() -> Self {
        Self(draw_token(b"agent-session", MAX_TOKEN_CHARS))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for SessionId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("not a session id"))
    }
}

/// Reads a request's `session` leniently: missing | malformed -> `None`, so the hub can answer the refusal instead
/// of dropping the line unanswered.
pub(crate) fn lenient<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<SessionId>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .and_then(SessionId::parse))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_lowercase_letters_and_digits_of_8_to_32() {
        for good in ["abcd1234", "k7m2q7xd2a90", &"a".repeat(32)] {
            assert!(SessionId::parse(good).is_some(), "{good}");
        }
        for bad in [
            "",
            "abc1234",
            &"a".repeat(33),
            "ABCD1234",
            "abcd-1234",
            "abcd 1234",
            "../../etc",
            "ħħħħħħħħ",
        ] {
            assert!(SessionId::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn drawn_ids_are_valid_and_distinct() {
        let a = SessionId::draw();
        let b = SessionId::draw();
        assert!(SessionId::parse(a.as_str()).is_some());
        assert_ne!(a, b);
    }
}
