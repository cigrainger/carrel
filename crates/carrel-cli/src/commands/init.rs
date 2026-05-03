//! `carrel init` implementation.

use std::collections::BTreeMap;
use std::fs;

use carrel_core::identity::{DeviceCert, Keypair};
use carrel_store::Store;
use carrel_store::keystore::Keystore;
use clap::Args;
use cozo::{DataValue, Validity};
use dialoguer::Password;

use crate::config::Context;
use crate::error::{CliError, Result};

/// Initialize a fresh Carrel data directory.
#[derive(Debug, Args)]
pub struct InitCommand {
    /// Passphrase for non-interactive tests and scripts.
    #[arg(long, hide = true)]
    pub passphrase: Option<String>,
}

/// Run `carrel init`.
pub fn run(context: &Context, command: &InitCommand) -> Result<()> {
    if context.paths.is_initialized() {
        return Err(CliError::user(format!(
            "Carrel is already initialized at {}",
            context.data_dir.display()
        )));
    }

    let passphrase = match &command.passphrase {
        Some(passphrase) => {
            eprintln!("warning: --passphrase is intended for tests and scripts");
            passphrase.clone()
        }
        None => Password::new()
            .with_prompt("Choose a passphrase for your master key")
            .with_confirmation("Confirm passphrase", "Passphrases do not match")
            .interact()?,
    };

    eprintln!("Generating master keypair...");
    let master = Keypair::generate();
    eprintln!("Generating device keypair...");
    let device = Keypair::generate();
    let cert = DeviceCert::new(&master, device.public(), Some("this device".to_string()));

    fs::create_dir_all(&context.paths.root)
        .map_err(|source| CliError::io(&context.paths.root, source))?;
    fs::create_dir_all(&context.paths.blobs)
        .map_err(|source| CliError::io(&context.paths.blobs, source))?;
    fs::create_dir_all(&context.paths.keys)
        .map_err(|source| CliError::io(&context.paths.keys, source))?;

    eprintln!("Saving keys...");
    let keystore = Keystore::open(&context.paths.keys);
    keystore.save_master(&master, &passphrase)?;
    keystore.save_device(&device)?;

    eprintln!("Creating store...");
    let store = Store::open(&context.paths.store)?;
    eprintln!("Applying schema migrations...");
    store.migrate()?;
    write_identity_rows(&store, &master, &device, &cert)?;
    write_default_config(context)?;

    println!("Initialized at {}", context.data_dir.display());
    println!("Master pubkey: {}", master.public());
    println!("Device pubkey: {}", device.public());
    println!();
    println!("Save your master pubkey somewhere safe. It is your identity.");

    Ok(())
}

fn write_identity_rows(
    store: &Store,
    master: &Keypair,
    device: &Keypair,
    cert: &DeviceCert,
) -> Result<()> {
    let authorized_at = DataValue::Validity(Validity::from((cert.authorized_at_micros, true)));
    let added_at = authorized_at.clone();
    let device_cert = cert.to_bytes().map_err(|source| {
        CliError::internal(format!("failed to serialize device cert: {source}"))
    })?;

    store.query_with_params(
        r#"
        {
            ?[pubkey, pet_name, self_described_name, is_self, added_at, last_seen] :=
                pubkey = $master_pubkey,
                pet_name = null,
                self_described_name = null,
                is_self = true,
                added_at = $added_at,
                last_seen = null
            :put peer {pubkey => pet_name, self_described_name, is_self, added_at, last_seen}
        }
        {
            ?[pubkey, pet_name, self_described_name, is_self, added_at, last_seen] :=
                pubkey = $device_pubkey,
                pet_name = 'this device',
                self_described_name = null,
                is_self = true,
                added_at = $added_at,
                last_seen = null
            :put peer {pubkey => pet_name, self_described_name, is_self, added_at, last_seen}
        }
        {
            ?[master_pubkey, device_pubkey, authorized_at, revoked_at, device_name, device_cert] :=
                master_pubkey = $master_pubkey,
                device_pubkey = $device_pubkey,
                authorized_at = $authorized_at,
                revoked_at = null,
                device_name = 'this device',
                device_cert = $device_cert
            :put device_authorization {master_pubkey, device_pubkey => authorized_at, revoked_at, device_name, device_cert}
        }
        "#,
        BTreeMap::from([
            (
                "master_pubkey".to_string(),
                DataValue::Bytes(master.public().as_bytes().to_vec()),
            ),
            (
                "device_pubkey".to_string(),
                DataValue::Bytes(device.public().as_bytes().to_vec()),
            ),
            ("added_at".to_string(), added_at),
            ("authorized_at".to_string(), authorized_at),
            ("device_cert".to_string(), DataValue::Bytes(device_cert)),
        ]),
    )?;

    Ok(())
}

fn write_default_config(context: &Context) -> Result<()> {
    let config = r#"[storage]

[ui]
theme = "auto"
font = "source-serif-4"
reading_size_px = 18

[keymap]
"#;

    fs::write(&context.paths.config, config)
        .map_err(|source| CliError::io(&context.paths.config, source))
}
