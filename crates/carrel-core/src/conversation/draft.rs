//! Unsigned installation-local composition; these types are never wire facts.
use crate::{identity::PublicKey, sharing::ShareError};
use serde::{Deserialize, Serialize};

/// Exact editable text and retry token supplied by the local composer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyDraftInput {
    /// Unnormalized text, including empty and whitespace-only composition.
    pub body: String,
    /// An earlier accepted reply in this conversation.
    pub parent: Option<String>,
    /// Stable local retry token, replaced after a confirmed send.
    pub request_id: String,
}

impl ReplyDraftInput {
    /// Validate portable bounds without requiring a nonempty sendable body.
    pub fn verify(&self) -> Result<(), ShareError> {
        if self.body.len() > 64 * 1024
            || self.request_id.is_empty()
            || self.request_id.len() > 128
            || !self
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
            || self
                .parent
                .as_ref()
                .is_some_and(|id| !crate::annotation::note::valid_revision_id(id))
        {
            return Err(ShareError::Invalid);
        }
        Ok(())
    }
}

/// Persisted local composition and its installation's operational retry identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyDraft {
    /// Exact unnormalized text.
    pub body: String,
    /// Optional accepted parent in the original conversation.
    pub parent: Option<String>,
    /// Stable token retained through unsuccessful saves and sends.
    pub request_id: String,
    /// Local operational key; recovery replaces this without signing anything.
    pub retry_device: PublicKey,
}

impl ReplyDraft {
    /// Editable input without permitting the webview to select a device key.
    pub fn input(&self) -> ReplyDraftInput {
        ReplyDraftInput {
            body: self.body.clone(),
            parent: self.parent.clone(),
            request_id: self.request_id.clone(),
        }
    }

    /// Validate composition and the retry-device public key.
    pub fn verify(&self) -> Result<(), ShareError> {
        self.input().verify()?;
        self.retry_device
            .validate()
            .map_err(|_| ShareError::Invalid)
    }
}

/// A compare-and-swap token and its live draft, or a retained clear tombstone.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyDraftSnapshot {
    /// Nonnegative local counter. Native DTOs encode this as a decimal string.
    pub revision: i64,
    /// Absence represents a cleared slot; revision zero represents no stored row.
    pub draft: Option<ReplyDraft>,
}

impl ReplyDraftSnapshot {
    /// Validate a snapshot without authorizing a send or requiring an endpoint.
    pub fn verify(&self) -> Result<(), ShareError> {
        if self.revision < 0 || (self.revision == 0 && self.draft.is_some()) {
            return Err(ShareError::Invalid);
        }
        if let Some(draft) = &self.draft {
            draft.verify()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn input(body: &str) -> ReplyDraftInput {
        ReplyDraftInput {
            body: body.into(),
            parent: None,
            request_id: "retry-1._".into(),
        }
    }

    #[test]
    fn composition_keeps_empty_whitespace_unicode_and_exact_byte_bounds() {
        for body in ["", " \n\t", "A thought — 文\nwith context"] {
            input(body).verify().unwrap();
        }
        input(&"文".repeat(21_845)).verify().unwrap();
        assert!(input(&"文".repeat(21_846)).verify().is_err());
        input(&"x".repeat(65_536)).verify().unwrap();
        assert!(input(&"x".repeat(65_537)).verify().is_err());
        for token in ["", "contains a space", "文", &"x".repeat(129)] {
            let mut draft = input("body");
            draft.request_id = token.into();
            assert!(draft.verify().is_err());
        }
        let mut draft = input("body");
        draft.parent = Some("a".repeat(64));
        draft.verify().unwrap();
        draft.parent = Some("not-a-reply".into());
        assert!(draft.verify().is_err());
    }

    #[test]
    fn live_drafts_require_a_positive_persisted_revision() {
        let draft = ReplyDraft {
            body: String::new(),
            parent: None,
            request_id: "retry".into(),
            retry_device: *crate::identity::Keypair::from_seed(&[81; 32]).public(),
        };
        for revision in [i64::MIN, -1, 0, 1, i64::MAX] {
            let snapshot = ReplyDraftSnapshot {
                revision,
                draft: Some(draft.clone()),
            };
            assert_eq!(snapshot.verify().is_ok(), revision > 0);
        }
    }

    proptest! {
        #[test]
        fn revision_tokens_never_depend_on_wall_time(revision in any::<i64>()) {
            let snapshot = ReplyDraftSnapshot { revision, draft: None };
            prop_assert_eq!(snapshot.verify().is_ok(), revision >= 0);
        }

        #[test]
        fn valid_composition_retains_exact_unicode(body in ".{0,128}") {
            let draft = ReplyDraft {
                body: body.clone(), parent: None, request_id: "retry".into(),
                retry_device: *crate::identity::Keypair::from_seed(&[81; 32]).public(),
            };
            let snapshot = ReplyDraftSnapshot { revision: 1, draft: Some(draft) };
            snapshot.verify().unwrap();
            let encoded = serde_json::to_vec(&snapshot).unwrap();
            let decoded: ReplyDraftSnapshot = serde_json::from_slice(&encoded).unwrap();
            prop_assert_eq!(&decoded, &snapshot);
            prop_assert_eq!(decoded.draft.unwrap().body, body);
        }
    }
}
