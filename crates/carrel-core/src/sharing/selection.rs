//! Local audience review and send intent. These labels never enter a social proof.
use super::{MAX_SHARE_NOTE_BYTES, ShareError};
use crate::{conversation::validate_recipients, identity::PublicKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Whether recipients deliberately meet or receive independent vouches.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SendMode {
    /// One immutable audience and shared reply history.
    Conversation,
    /// One independent proof and conversation for each recipient.
    Separately,
}

/// A local group membership snapshot actually reviewed before sending.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedGroup {
    /// Local group ID, never its private display name.
    pub id: String,
    /// Exact reviewed members; later edits require another review.
    pub members: Vec<PublicKey>,
}

/// A bounded local selection, distinct from any signed audience capability.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudienceSelection {
    /// Individually selected masters.
    pub people: Vec<PublicKey>,
    /// Reviewed local group snapshots.
    pub groups: Vec<SelectedGroup>,
}
impl AudienceSelection {
    /// Normalize equivalent choices while rejecting contradictory group snapshots.
    pub fn normalized(mut self) -> Result<Self, ShareError> {
        if self.people.len() > 16 || self.groups.len() > 16 {
            return Err(ShareError::Invalid);
        }
        self.people.sort();
        self.people.dedup();
        for group in &mut self.groups {
            if group.id.is_empty()
                || group.id.len() > 128
                || !group
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
                || group.members.is_empty()
                || group.members.len() > 16
            {
                return Err(ShareError::Invalid);
            }
            group.members.sort();
            group.members.dedup();
        }
        self.groups.sort_by(|a, b| a.id.cmp(&b.id));
        if self
            .groups
            .windows(2)
            .any(|pair| pair[0].id == pair[1].id && pair[0] != pair[1])
        {
            return Err(ShareError::Invalid);
        }
        self.groups.dedup();
        Ok(self)
    }
    /// Union overlapping choices; every recipient appears once in canonical order.
    pub fn recipients(&self, author: PublicKey) -> Result<Vec<PublicKey>, ShareError> {
        let normalized = self.clone().normalized()?;
        let keys: Vec<_> = normalized
            .people
            .into_iter()
            .chain(
                normalized
                    .groups
                    .into_iter()
                    .flat_map(|group| group.members),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        validate_recipients(&keys, author)?;
        Ok(keys)
    }
}

/// Exact device-local send intent retained for atomic, privacy-preserving retries.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SendDraft {
    /// Stable composer token.
    pub request_id: String,
    /// Local archived article identity.
    pub item_id: String,
    /// Optional deliberately entered opening note.
    pub note: Option<String>,
    /// Reviewed selection; names and aliases remain outside signed wire records.
    pub selection: AudienceSelection,
    /// Explicit recipient visibility and conversation choice.
    pub mode: SendMode,
}
impl SendDraft {
    /// Canonical local intent, excluding incidental order and duplicate selections.
    pub fn normalized(mut self) -> Result<Self, ShareError> {
        self.selection = self.selection.normalized()?;
        if self.item_id.is_empty()
            || self.item_id.len() > 256
            || self
                .note
                .as_ref()
                .is_some_and(|note| note.trim().is_empty() || note.len() > MAX_SHARE_NOTE_BYTES)
        {
            return Err(ShareError::Invalid);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Keypair;
    use proptest::prelude::*;
    fn key(n: u8) -> PublicKey {
        *Keypair::from_seed(&[n; 32]).public()
    }

    proptest! {
        #[test]
        fn overlapping_selections_have_a_stable_unique_audience(a in prop::collection::vec(1u8..8,1..8), b in prop::collection::vec(1u8..8,1..8)) {
            let selection=AudienceSelection {people:a.iter().copied().map(key).collect(),groups:vec![SelectedGroup{id:"essays".into(),members:b.iter().copied().map(key).collect()}]};
            let recipients=selection.recipients(key(0)).unwrap();
            let expected:Vec<_>=a.into_iter().chain(b).map(key).collect::<BTreeSet<_>>().into_iter().collect();
            prop_assert_eq!(recipients,expected);
            let normalized=selection.normalized().unwrap();
            prop_assert_eq!(normalized.clone().normalized().unwrap(),normalized);
        }
    }
    #[test]
    fn empty_self_invalid_oversized_and_conflicting_choices_are_refused() {
        assert!(AudienceSelection::default().recipients(key(0)).is_err());
        assert!(
            AudienceSelection {
                people: vec![key(0)],
                groups: vec![]
            }
            .recipients(key(0))
            .is_err()
        );
        assert!(
            AudienceSelection {
                people: (1..=17).map(key).collect(),
                groups: vec![]
            }
            .recipients(key(0))
            .is_err()
        );
        let group = SelectedGroup {
            id: "essays".into(),
            members: vec![key(1)],
        };
        let mut different = group.clone();
        different.members = vec![key(2)];
        assert!(
            AudienceSelection {
                people: vec![],
                groups: vec![group, different]
            }
            .normalized()
            .is_err()
        );
        assert!(
            AudienceSelection {
                people: vec![],
                groups: vec![SelectedGroup {
                    id: "bad/id".into(),
                    members: vec![key(1)]
                }]
            }
            .normalized()
            .is_err()
        );
    }

    #[test]
    fn recipient_limit_applies_to_the_union_and_both_modes_are_explicit() {
        let mut selection = AudienceSelection {
            people: vec![],
            groups: vec![SelectedGroup {
                id: "full".into(),
                members: (1..=16).map(key).collect(),
            }],
        };
        assert_eq!(selection.recipients(key(0)).unwrap().len(), 16);
        selection.groups.push(SelectedGroup {
            id: "one-more".into(),
            members: vec![key(17)],
        });
        assert!(selection.recipients(key(0)).is_err());
        assert_eq!(
            serde_json::to_string(&SendMode::Conversation).unwrap(),
            "\"conversation\""
        );
        assert_eq!(
            serde_json::to_string(&SendMode::Separately).unwrap(),
            "\"separately\""
        );
        assert!(serde_json::from_str::<SendMode>("\"broadcast\"").is_err());
    }
}
