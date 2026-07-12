//! Signing primitives — thin delegations to `heartwood-common`'s
//! `ledger-backend`, where every secret-dependent curve operation (pubkey
//! derivation, BIP-340 signing, and NIP-44's ECDH) runs on the secure
//! element's cx syscalls rather than in app RAM. Correctness is pinned by the
//! e2e proof, which verifies everything host-side with k256.

use heartwood_common::derive::{ledger_pubkey_from_secret, ledger_sign_bip340};

/// Derive the 32-byte x-only public key from a 32-byte master secret.
/// `None` if the secret is not a valid secp256k1 scalar.
pub fn pubkey(seed: &[u8; 32]) -> Option<[u8; 32]> {
    ledger_pubkey_from_secret(seed).ok()
}

/// BIP-340 Schnorr sign a 32-byte message (a Nostr event id) with the master
/// secret, on the secure element. Nonces are deterministic (key + message, no
/// RNG), matching the radio-off ESP signers.
pub fn sign(seed: &[u8; 32], message: &[u8; 32]) -> Option<[u8; 64]> {
    ledger_sign_bip340(seed, message).ok()
}
