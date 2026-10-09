//! Preserve legacy data while establishing verifiable device trust.

use std::collections::BTreeMap;

use carrel_core::identity::DeviceCert;
use cozo::{DataValue, DbInstance, ScriptMutability, Validity};
use time::OffsetDateTime;

use crate::identity::{bytes, certificate_params, public_key};
use crate::{Result, schema};

pub fn up(db: &DbInstance) -> Result<()> {
    let run = |script: &str| db.run_script(script, BTreeMap::new(), ScriptMutability::Mutable);
    for (name, script) in schema::SELF_IDENTITY_RELATIONS {
        if !run("::relations")?
            .rows
            .iter()
            .any(|row| row.first() == Some(&DataValue::from(*name)))
        {
            run(script)?;
        }
    }
    for script in schema::SELF_IDENTITY_GUARDS {
        run(script)?;
    }
    let legacy = run(
        "?[master, device, at, revoked, name, cert] := *device_authorization{master_pubkey: master, device_pubkey: device, authorized_at: at, revoked_at: revoked, device_name: name, device_cert: cert}",
    )?;
    for row in legacy.rows {
        // Legacy unsigned revocation hints stay local and fail closed. They must
        // never become fabricated proofs, even if the old certificate is bad.
        if row[3] != DataValue::Null && public_key(&row[0]).is_ok() && public_key(&row[1]).is_ok() {
            db.run_script(
                "?[master_pubkey, device_pubkey, reason] <- [[$master, $device, 'legacy revocation lacks signed proof']] :put device_local_block {master_pubkey, device_pubkey => reason}",
                BTreeMap::from([("master".into(), row[0].clone()), ("device".into(), row[1].clone())]),
                ScriptMutability::Mutable,
            )?;
        }
        let Ok(raw) = bytes(&row[5]) else {
            continue;
        };
        let Ok(cert) = DeviceCert::from_bytes(raw) else {
            continue;
        };
        let signed_name = cert
            .device_name
            .as_deref()
            .map_or(DataValue::Null, DataValue::from);
        if row[0] != DataValue::Bytes(cert.master_pubkey.to_bytes().to_vec())
            || row[1] != DataValue::Bytes(cert.device_pubkey.to_bytes().to_vec())
            || row[2] != DataValue::Validity(Validity::from((cert.authorized_at_micros, true)))
            || row[4] != signed_name
            || cert.master_pubkey == cert.device_pubkey
        {
            continue;
        }
        db.run_script(
            "?[master_pubkey, device_pubkey, device_cert] <- [[$master, $device, $cert]] :put device_certificate {master_pubkey, device_pubkey => device_cert}",
            certificate_params(&cert)?, ScriptMutability::Mutable,
        )?;
    }
    let now = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000) as i64;
    db.run_script(
        "?[version, applied_at, description] <- [[4, $now, 'verified self-device identity and revocation proofs']] :put schema_version {version => applied_at, description}",
        BTreeMap::from([("now".into(), DataValue::Validity(Validity::from((now, true))))]),
        ScriptMutability::Mutable,
    )?;
    Ok(())
}
