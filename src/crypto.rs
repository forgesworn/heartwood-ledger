//! Signing primitives — the same pure-Rust k256 BIP-340 path the ESP8266
//! firmware ships (`esp8266-firmware/src/crypto.rs`), so signatures are
//! byte-identical across signer hardware.
//!
//! PoC note: k256 runs the curve maths in app RAM rather than through the
//! secure element's cx syscalls. Swapping `pubkey`/`sign` (and the ECDH inside
//! `heartwood_common::nip44`) onto `cx_ecschnorr_sign_no_throw` /
//! `cx_ecdh_no_throw` is the planned hardening step before any listing review —
//! the seams are exactly these two functions plus a `ledger-backend` feature in
//! `heartwood-common`.

use k256::schnorr::SigningKey;

/// Derive the 32-byte x-only public key from a 32-byte master secret.
/// `None` if the secret is not a valid secp256k1 scalar.
pub fn pubkey(seed: &[u8; 32]) -> Option<[u8; 32]> {
    let sk = SigningKey::from_bytes(seed).ok()?;
    let bytes = sk.verifying_key().to_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(bytes.as_ref());
    Some(out)
}

/// BIP-340 Schnorr sign a 32-byte message (a Nostr event id) with the master
/// secret. Returns the 64-byte signature.
///
/// Signs the id **directly** and **deterministically**:
/// - Nostr signs the event id itself, so we must NOT re-hash it — hence
///   `sign_prehash_with_aux_rand`, which signs the 32-byte digest as-is.
/// - `aux_rand = 0` makes the BIP-340 nonce deterministic (derived from the key
///   + message), matching the radio-off ESP signers.
pub fn sign(seed: &[u8; 32], message: &[u8; 32]) -> Option<[u8; 64]> {
    let sk = SigningKey::from_bytes(seed).ok()?;
    let sig = sk.sign_prehash_with_aux_rand(message, &[0u8; 32]).ok()?;
    let mut out = [0u8; 64];
    out.copy_from_slice(sig.to_bytes().as_ref());
    Some(out)
}
