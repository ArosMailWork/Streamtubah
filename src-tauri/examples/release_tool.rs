//! Release signing for the self-updater.
//!
//!   cargo run --example release_tool -- keygen            create the signing key (once)
//!   cargo run --example release_tool -- sign <file>       print the base64 signature of a file
//!   cargo run --example release_tool -- pubkey            print the public key to embed in updater.rs
//!
//! The private key lives outside the repository in `%USERPROFILE%\.streamtubah\update-signing.key`
//! (override with STREAMTUBAH_SIGNING_KEY). Back it up: without it, installed copies can no longer
//! accept automatic updates (users would have to download the full package by hand).

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use std::path::PathBuf;

fn key_path() -> PathBuf {
    std::env::var_os("STREAMTUBAH_SIGNING_KEY").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("USERPROFILE").expect("USERPROFILE")).join(".streamtubah").join("update-signing.key")
    })
}

fn load() -> SigningKey {
    let raw = std::fs::read_to_string(key_path()).unwrap_or_else(|e| panic!("cannot read {}: {e}", key_path().display()));
    let bytes = base64::engine::general_purpose::STANDARD.decode(raw.trim()).expect("key file is not base64");
    SigningKey::from_bytes(&bytes.try_into().expect("key must be 32 bytes"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let b64 = base64::engine::general_purpose::STANDARD;
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let path = key_path();
            if path.exists() {
                eprintln!("{} already exists; refusing to overwrite", path.display());
                std::process::exit(1);
            }
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).expect("OS random source");
            let key = SigningKey::from_bytes(&seed);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b64.encode(seed)).unwrap();
            println!("private key: {}", path.display());
            println!("public key:  {}", b64.encode(key.verifying_key().to_bytes()));
        }
        Some("pubkey") => println!("{}", b64.encode(load().verifying_key().to_bytes())),
        Some("sign") => {
            let file = args.get(1).expect("usage: sign <file>");
            let data = std::fs::read(file).unwrap_or_else(|e| panic!("cannot read {file}: {e}"));
            println!("{}", b64.encode(load().sign(&data).to_bytes()));
        }
        _ => {
            eprintln!("usage: release_tool keygen | pubkey | sign <file>");
            std::process::exit(2);
        }
    }
}
