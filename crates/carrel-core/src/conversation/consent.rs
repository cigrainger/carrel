//! Private consent operations share one causal domain, independent of clocks.
use super::ThreadScope;
use crate::private_state::{StateKey, StateKind, StateRevision, StateRevisionError, StateValue};
use crate::sharing::ShareError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Why this desktop admitted a root. No private follow list is disclosed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionBasis {
    /// All other participants were explicitly followed at admission.
    AllFollowed,
    /// An unfamiliar group was allowed by the observed preference.
    IntroductionsAllowed,
    /// The exact root was accepted before the consent migration boundary.
    LegacyAccepted,
}

/// Cumulative operations in the singleton private conversation register.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConversationControl {
    /// Preference for future unfamiliar group roots.
    Policy {
        /// Whether future unfamiliar groups may be admitted.
        allow_unfollowed_groups: bool,
    },
    /// Original recipient consent, bound to the complete signed root.
    Admit {
        /// Exact signed root and original audience.
        scope: ThreadScope,
        /// Local admission evidence, without disclosing follows.
        basis: AdmissionBasis,
    },
    /// Permanent refusal of this exact root.
    Decline {
        /// Exact root permanently refused by the recipient.
        scope: ThreadScope,
    },
    /// Permanent local departure, without changing the original audience.
    Leave {
        /// Exact root permanently left by the recipient.
        scope: ThreadScope,
    },
}

impl ConversationControl {
    /// Validate scope even when its social root has not arrived.
    pub fn verify(&self) -> Result<(), ShareError> {
        if let Some(scope) = self.scope() {
            scope.verify()?;
        }
        Ok(())
    }

    /// Exact root, absent only for a preference operation.
    pub fn scope(&self) -> Option<&ThreadScope> {
        match self {
            Self::Policy { .. } => None,
            Self::Admit { scope, .. } | Self::Decline { scope } | Self::Leave { scope } => {
                Some(scope)
            }
        }
    }
}

/// The recipient's private participation, independent of author withdrawal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationParticipation {
    /// No signed admission yet, or control ancestry remains unresolved.
    Pending,
    /// New discussion is permitted.
    Admitted,
    /// Explicit or concurrent-policy refusal; cannot silently rejoin.
    Declined,
    /// Explicit departure; retained history remains available.
    Left,
}

/// A resolved exact-root decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationDecision {
    /// Original immutable scope.
    pub scope: ThreadScope,
    /// Private effective participation.
    pub participation: ConversationParticipation,
}

/// Rebuildable interpretation of the complete signed control history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationControls {
    /// Concurrent preference conflicts resolve off; absence defaults on.
    pub allow_unfollowed_groups: bool,
    /// Decisions indexed by root identity, never article identity.
    pub conversations: BTreeMap<String, ConversationDecision>,
}

/// All preferences and root decisions must share these causal parents.
pub fn conversation_control_key() -> StateKey {
    StateKey {
        kind: StateKind::ConversationControl,
        target: "global".into(),
    }
}

/// Resolve a complete DAG, rejecting missing ancestry and scope equivocation.
///
/// A terminal decision never disappears when another operation supersedes its
/// head. Historical off/admission concurrency also remains after re-enabling.
pub fn resolve_conversation_controls(
    records: &[StateRevision],
) -> Result<ConversationControls, StateRevisionError> {
    let mut nodes = BTreeMap::new();
    for record in records {
        record.verify()?;
        if record.key != conversation_control_key() {
            return Err(StateRevisionError::Content);
        }
        if let Some(previous) = nodes.insert(record.revision_id.clone(), record)
            && previous != record
        {
            return Err(StateRevisionError::Identity);
        }
    }
    let needs_reachability = nodes.values().any(|record| {
        matches!(
            &record.value,
            StateValue::ConversationControl(ConversationControl::Policy { .. })
        )
    });
    let mut ancestors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    while ancestors.len() < nodes.len() {
        let mut progressed = false;
        for (id, record) in &nodes {
            if ancestors.contains_key(id)
                || record.parents.iter().any(|p| !ancestors.contains_key(p))
            {
                continue;
            }
            let mut seen = BTreeSet::new();
            // Only policy conflicts need reachability. Default-on histories
            // still traverse every parent, without storing a quadratic closure.
            if needs_reachability {
                for parent in &record.parents {
                    seen.insert(parent.clone());
                    seen.extend(ancestors[parent].iter().cloned());
                }
            }
            ancestors.insert(id.clone(), seen);
            progressed = true;
        }
        if !progressed {
            return Err(StateRevisionError::MissingParents);
        }
    }
    fn operation(record: &StateRevision) -> &ConversationControl {
        match &record.value {
            StateValue::ConversationControl(value) => value,
            _ => unreachable!("verified control key requires a control payload"),
        }
    }
    let policies: Vec<_> = nodes
        .iter()
        .filter_map(|(id, r)| match operation(r) {
            ConversationControl::Policy {
                allow_unfollowed_groups,
            } => Some((id, *allow_unfollowed_groups)),
            _ => None,
        })
        .collect();
    let policy_at = |cut: &BTreeSet<String>| {
        policies
            .iter()
            .filter(|(id, _)| cut.contains(*id))
            .filter(|(id, _)| {
                !policies
                    .iter()
                    .any(|(other, _)| cut.contains(*other) && ancestors[*other].contains(*id))
            })
            .all(|(_, allow)| *allow)
    };
    let mut result = ConversationControls {
        allow_unfollowed_groups: policy_at(&nodes.keys().cloned().collect()),
        conversations: BTreeMap::new(),
    };
    for record in nodes.values() {
        if let Some(scope) = operation(record).scope() {
            if let Some(previous) = result.conversations.get(&scope.share_id) {
                if previous.scope != *scope {
                    return Err(StateRevisionError::Content);
                }
            } else {
                result.conversations.insert(
                    scope.share_id.clone(),
                    ConversationDecision {
                        scope: scope.clone(),
                        participation: ConversationParticipation::Pending,
                    },
                );
            }
        }
    }
    for decision in result.conversations.values_mut() {
        let thread: Vec<_> = nodes
            .iter()
            .filter(|(_, r)| operation(r).scope() == Some(&decision.scope))
            .collect();
        let admissions: Vec<_> = thread
            .iter()
            .filter_map(|(id, r)| match operation(r) {
                ConversationControl::Admit { basis, .. } => Some((*id, *basis)),
                _ => None,
            })
            .collect();
        for (id, basis) in &admissions {
            if *basis == AdmissionBasis::IntroductionsAllowed && !policy_at(&ancestors[*id]) {
                return Err(StateRevisionError::Content);
            }
        }
        let veto = admissions.iter().any(|(admit, basis)| {
            *basis == AdmissionBasis::IntroductionsAllowed
                && policies.iter().any(|(off, allow)| {
                    !allow
                        && !ancestors[*admit].contains(*off)
                        && !ancestors[*off].contains(*admit)
                        && !admissions
                            .iter()
                            .any(|(known, _)| ancestors[*off].contains(*known))
                })
        });
        decision.participation = if thread
            .iter()
            .any(|(_, r)| matches!(operation(r), ConversationControl::Leave { .. }))
        {
            ConversationParticipation::Left
        } else if thread
            .iter()
            .any(|(_, r)| matches!(operation(r), ConversationControl::Decline { .. }))
            || veto
        {
            ConversationParticipation::Declined
        } else if !admissions.is_empty() {
            ConversationParticipation::Admitted
        } else {
            ConversationParticipation::Pending
        };
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Keypair;
    use proptest::prelude::*;

    fn scope(id: u8) -> ThreadScope {
        let mut recipients = vec![
            *Keypair::from_seed(&[2; 32]).public(),
            *Keypair::from_seed(&[3; 32]).public(),
        ];
        recipients.sort();
        ThreadScope {
            share_id: format!("{id:064x}"),
            root_digest: format!("{:064x}", id + 1),
            sharer: *Keypair::from_seed(&[1; 32]).public(),
            recipients,
        }
    }
    fn event(op: ConversationControl, parents: &[&StateRevision], at: i64) -> StateRevision {
        let mut ids: Vec<_> = parents.iter().map(|r| r.revision_id.clone()).collect();
        ids.sort();
        StateRevision::sign(
            conversation_control_key(),
            StateValue::ConversationControl(op),
            ids,
            at,
            &Keypair::from_seed(&[4; 32]),
        )
        .unwrap()
    }
    fn admit(s: &ThreadScope, basis: AdmissionBasis, parents: &[&StateRevision]) -> StateRevision {
        event(
            ConversationControl::Admit {
                scope: s.clone(),
                basis,
            },
            parents,
            i64::MAX,
        )
    }
    fn policy(allow: bool, parents: &[&StateRevision]) -> StateRevision {
        event(
            ConversationControl::Policy {
                allow_unfollowed_groups: allow,
            },
            parents,
            i64::MIN,
        )
    }
    fn state(records: &[StateRevision], s: &ThreadScope) -> ConversationParticipation {
        resolve_conversation_controls(records)
            .unwrap()
            .conversations[&s.share_id]
            .participation
    }

    #[test]
    fn cumulative_policy_and_duplicate_admission_preserve_existing_roots() {
        assert!(
            resolve_conversation_controls(&[])
                .unwrap()
                .allow_unfollowed_groups
        );
        let s = scope(10);
        let first = admit(&s, AdmissionBasis::IntroductionsAllowed, &[]);
        let off = policy(false, &[&first]);
        let duplicate = event(
            ConversationControl::Admit {
                scope: s.clone(),
                basis: AdmissionBasis::IntroductionsAllowed,
            },
            &[],
            12,
        );
        let history = vec![off, duplicate, first];
        assert!(
            !resolve_conversation_controls(&history)
                .unwrap()
                .allow_unfollowed_groups
        );
        assert_eq!(state(&history, &s), ConversationParticipation::Admitted);
    }

    #[test]
    fn default_on_diamonds_keep_terminal_choices_and_require_complete_ancestry() {
        let first = admit(&scope(11), AdmissionBasis::IntroductionsAllowed, &[]);
        let left = admit(&scope(12), AdmissionBasis::AllFollowed, &[&first]);
        let right = admit(&scope(13), AdmissionBasis::LegacyAccepted, &[&first]);
        let leave = event(
            ConversationControl::Leave { scope: scope(11) },
            &[&left, &right],
            2,
        );
        let decline = event(
            ConversationControl::Decline { scope: scope(12) },
            &[&leave],
            3,
        );
        let later = admit(&scope(11), AdmissionBasis::AllFollowed, &[&decline]);
        let mut history = vec![first, left, right, leave, decline, later];
        let resolved = resolve_conversation_controls(&history).unwrap();
        assert!(resolved.allow_unfollowed_groups);
        for (id, expected) in [
            (11, ConversationParticipation::Left),
            (12, ConversationParticipation::Declined),
            (13, ConversationParticipation::Admitted),
        ] {
            assert_eq!(
                resolved.conversations[&scope(id).share_id].participation,
                expected
            );
        }
        history.reverse();
        history.push(history[0].clone());
        assert_eq!(resolve_conversation_controls(&history).unwrap(), resolved);
        history.retain(|record| !record.parents.is_empty());
        assert!(matches!(
            resolve_conversation_controls(&history),
            Err(StateRevisionError::MissingParents)
        ));
    }

    #[test]
    fn concurrent_off_is_terminal_but_reenable_allows_a_new_root() {
        let s = scope(20);
        let admission = admit(&s, AdmissionBasis::IntroductionsAllowed, &[]);
        let off = policy(false, &[]);
        let on = policy(true, &[&admission, &off]);
        let duplicate = admit(&s, AdmissionBasis::IntroductionsAllowed, &[&on]);
        let other = scope(21);
        let fresh = admit(&other, AdmissionBasis::IntroductionsAllowed, &[&on]);
        let history = vec![fresh, duplicate, on, off, admission];
        assert!(
            resolve_conversation_controls(&history)
                .unwrap()
                .allow_unfollowed_groups
        );
        assert_eq!(state(&history, &s), ConversationParticipation::Declined);
        assert_eq!(state(&history, &other), ConversationParticipation::Admitted);
    }

    #[test]
    fn known_and_legacy_admissions_ignore_concurrent_preference_changes() {
        for basis in [AdmissionBasis::AllFollowed, AdmissionBasis::LegacyAccepted] {
            let s = scope(30);
            assert_eq!(
                state(&[admit(&s, basis, &[]), policy(false, &[])], &s),
                ConversationParticipation::Admitted
            );
        }
    }

    #[test]
    fn terminal_operations_survive_later_policy_admission_and_clock_rollback() {
        for (op, expected) in [
            (
                ConversationControl::Decline { scope: scope(40) },
                ConversationParticipation::Declined,
            ),
            (
                ConversationControl::Leave { scope: scope(40) },
                ConversationParticipation::Left,
            ),
        ] {
            let stop = event(op, &[], i64::MAX);
            let on = policy(true, &[&stop]);
            let later = admit(&scope(40), AdmissionBasis::AllFollowed, &[&on]);
            assert_eq!(state(&[later, on, stop], &scope(40)), expected);
        }
    }

    #[test]
    fn concurrent_policies_use_causality_instead_of_current_control_heads() {
        let off = policy(false, &[]);
        let on = policy(true, &[]);
        let known = admit(&scope(50), AdmissionBasis::AllFollowed, &[&off, &on]);
        assert!(
            !resolve_conversation_controls(&[known.clone(), off.clone(), on.clone()])
                .unwrap()
                .allow_unfollowed_groups
        );
        let resolved = policy(true, &[&known]);
        assert!(
            resolve_conversation_controls(&[resolved, known, off, on])
                .unwrap()
                .allow_unfollowed_groups
        );
    }

    #[test]
    fn malformed_ancestry_scopes_and_admission_under_off_are_rejected() {
        let off = policy(false, &[]);
        let bad = admit(&scope(60), AdmissionBasis::IntroductionsAllowed, &[&off]);
        assert!(resolve_conversation_controls(std::slice::from_ref(&bad)).is_err());
        assert!(resolve_conversation_controls(&[bad, off]).is_err());
        let first = admit(&scope(60), AdmissionBasis::AllFollowed, &[]);
        let mut fork = scope(60);
        fork.root_digest = "a".repeat(64);
        assert!(
            resolve_conversation_controls(&[first, admit(&fork, AdmissionBasis::AllFollowed, &[])])
                .is_err()
        );
        assert!(
            StateRevision::sign(
                StateKey {
                    kind: StateKind::ConversationControl,
                    target: "other".into()
                },
                StateValue::ConversationControl(ConversationControl::Policy {
                    allow_unfollowed_groups: true
                }),
                vec![],
                0,
                &Keypair::from_seed(&[4; 32])
            )
            .is_err()
        );
        let mut encoded = scope(60).to_bytes().unwrap();
        encoded.push(0);
        assert!(ThreadScope::from_bytes(&encoded).is_err());
    }

    proptest! {
        #[test]
        fn delivery_order_duplicates_and_policy_heads_cannot_revive_a_decline(order in prop::collection::vec(any::<u64>(), 5)) {
            let s=scope(70);
            let a=admit(&s,AdmissionBasis::IntroductionsAllowed,&[]);
            let off=policy(false,&[]);
            let on=policy(true,&[&a,&off]);
            let later=admit(&s,AdmissionBasis::AllFollowed,&[&on]);
            let mut records=vec![a.clone(),off,on,later,a];
            let mut ranked:Vec<_>=records.drain(..).zip(order).collect();
            ranked.sort_by_key(|(_,rank)|*rank);
            let records:Vec<_>=ranked.into_iter().map(|(r,_)|r).collect();
            prop_assert_eq!(state(&records,&s),ConversationParticipation::Declined);
            prop_assert!(resolve_conversation_controls(&records).unwrap().allow_unfollowed_groups);
        }
    }
}
