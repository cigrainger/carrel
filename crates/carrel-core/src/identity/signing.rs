//! Canonical-byte signing helpers for Carrel facts.

use ciborium::value::{CanonicalValue, Value};
use serde::Serialize;

use super::keypair::{Keypair, PublicKey, Signature, VerifyError, verify};

/// Serialize a value to deterministic canonical CBOR bytes.
///
/// Values are first lowered into Ciborium's dynamic `Value`, all maps are
/// recursively sorted with RFC 8949 canonical key ordering, and the result is
/// written as CBOR. Signed fact structs should use this function for the exact
/// bytes that are covered by a signature.
pub fn canonical_bytes<T>(value: &T) -> Result<Vec<u8>, SerializeError>
where
    T: Serialize + ?Sized,
{
    let value = Value::serialized(value)
        .map(canonicalize_value)
        .map_err(|source| SerializeError::Value(source.to_string()))?;
    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&value, &mut bytes)
        .map_err(|source| SerializeError::Write(source.to_string()))?;
    Ok(bytes)
}

/// Sign a serializable value after converting it to canonical CBOR.
pub fn sign_canonical<T>(keypair: &Keypair, value: &T) -> Result<Signature, SerializeError>
where
    T: Serialize + ?Sized,
{
    Ok(keypair.sign(&canonical_bytes(value)?))
}

/// Verify a canonical-CBOR signature for a serializable value.
pub fn verify_canonical<T>(
    public_key: &PublicKey,
    value: &T,
    signature: &Signature,
) -> Result<(), CanonicalVerifyError>
where
    T: Serialize + ?Sized,
{
    let bytes = canonical_bytes(value)?;
    verify(public_key, &bytes, signature)?;
    Ok(())
}

fn canonicalize_value(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_value).collect()),
        Value::Map(entries) => {
            let mut entries = entries
                .into_iter()
                .map(|(key, value)| (canonicalize_value(key), canonicalize_value(value)))
                .collect::<Vec<_>>();

            entries.sort_by(|(left, _), (right, _)| {
                CanonicalValue::from(left.clone()).cmp(&CanonicalValue::from(right.clone()))
            });

            Value::Map(entries)
        }
        Value::Tag(tag, value) => Value::Tag(tag, Box::new(canonicalize_value(*value))),
        other => other,
    }
}

/// Errors returned while producing canonical bytes.
#[derive(Debug, thiserror::Error)]
pub enum SerializeError {
    /// The value could not be represented as a CBOR value.
    #[error("failed to build CBOR value: {0}")]
    Value(String),

    /// The canonical CBOR value could not be written to bytes.
    #[error("failed to write canonical CBOR: {0}")]
    Write(String),
}

/// Errors returned while verifying a canonical-CBOR signature.
#[derive(Debug, thiserror::Error)]
pub enum CanonicalVerifyError {
    /// The signed value could not be serialized to canonical bytes.
    #[error(transparent)]
    Serialize(#[from] SerializeError),

    /// The canonical bytes did not verify against the supplied signature.
    #[error(transparent)]
    Verify(#[from] VerifyError),
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use proptest::prelude::*;
    use serde::Serialize;

    use super::{canonical_bytes, sign_canonical, verify_canonical};
    use crate::identity::Keypair;

    #[test]
    fn canonical_bytes_are_stable_across_runs() {
        let value = BTreeMap::from([("item".to_string(), 1_i64), ("audience".to_string(), 2_i64)]);

        assert_eq!(
            canonical_bytes(&value).unwrap(),
            canonical_bytes(&value).unwrap()
        );
    }

    #[test]
    fn canonical_bytes_sort_map_keys() {
        let mut first = HashMap::new();
        first.insert("z", 1_i64);
        first.insert("a", 2_i64);
        first.insert("m", 3_i64);

        let mut second = HashMap::new();
        second.insert("m", 3_i64);
        second.insert("z", 1_i64);
        second.insert("a", 2_i64);

        assert_eq!(
            canonical_bytes(&first).unwrap(),
            canonical_bytes(&second).unwrap()
        );
    }

    #[test]
    fn sign_and_verify_canonical_value() {
        #[derive(Serialize)]
        struct Fact<'a> {
            item_id: &'a str,
            shared_at_micros: i64,
        }

        let keypair = Keypair::from_seed(&[9_u8; 32]);
        let fact = Fact {
            item_id: "item-1",
            shared_at_micros: 42,
        };
        let signature = sign_canonical(&keypair, &fact).unwrap();

        verify_canonical(keypair.public(), &fact, &signature).unwrap();
    }

    proptest! {
        #[test]
        fn equivalent_string_maps_have_equal_canonical_bytes(entries in prop::collection::btree_map("\\PC{0,16}", any::<i64>(), 0..32)) {
            let mut first = HashMap::new();
            let mut second = HashMap::new();

            for (key, value) in &entries {
                first.insert(key.clone(), *value);
            }

            for (key, value) in entries.iter().rev() {
                second.insert(key.clone(), *value);
            }

            prop_assert_eq!(canonical_bytes(&first).unwrap(), canonical_bytes(&second).unwrap());
        }
    }
}
