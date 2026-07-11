//! Signing primitives.
//!
//! BIP-340 signing goes through the OS's `cx_ecschnorr_sign_no_throw` — the
//! hardened, constant-time syscall the Bitcoin app uses for taproot — rather
//! than curve maths in app RAM. Correctness is pinned by the e2e proof, which
//! verifies every signature host-side with k256. The aux data is all-zero, so
//! nonces are deterministic (derived from key + message), matching the
//! radio-off ESP signers.
//!
//! Public-key derivation stays on k256 (`heartwood-common`'s backend needs it
//! for nsec-tree derivation and NIP-44 ECDH regardless); moving those onto cx
//! syscalls means giving `heartwood-common` a `ledger-backend` feature — the
//! remaining pre-review hardening step.

use core::mem::MaybeUninit;

use k256::schnorr::SigningKey;
use ledger_secure_sdk_sys::{
    cx_ecfp_init_private_key_no_throw, cx_ecfp_private_key_t, cx_ecschnorr_sign_no_throw,
    CX_CURVE_SECP256K1, CX_OK, CX_RND_PROVIDED, CX_SHA256,
};
use zeroize::Zeroize;

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
/// secret, on the secure element. Returns the 64-byte signature.
///
/// Nostr signs the event id itself — `CX_ECSCHNORR_BIP0340` takes the 32-byte
/// message as-is (no re-hashing). `CX_RND_PROVIDED` reads the aux data from
/// the signature buffer on entry; it is zeroed, so signing needs no RNG.
pub fn sign(seed: &[u8; 32], message: &[u8; 32]) -> Option<[u8; 64]> {
    const CX_ECSCHNORR_BIP0340: u32 = 0;
    let mut sig = [0u8; 64];
    let mut sig_len: usize = sig.len();
    let mut pvkey = MaybeUninit::<cx_ecfp_private_key_t>::uninit();
    unsafe {
        if cx_ecfp_init_private_key_no_throw(
            CX_CURVE_SECP256K1,
            seed.as_ptr(),
            seed.len(),
            pvkey.as_mut_ptr(),
        ) != CX_OK
        {
            return None;
        }
        let mut pvkey = pvkey.assume_init();
        let rc = cx_ecschnorr_sign_no_throw(
            &pvkey,
            CX_ECSCHNORR_BIP0340 | CX_RND_PROVIDED,
            CX_SHA256,
            message.as_ptr(),
            message.len(),
            sig.as_mut_ptr(),
            &mut sig_len,
        );
        pvkey.d.zeroize();
        if rc != CX_OK || sig_len != sig.len() {
            return None;
        }
    }
    Some(sig)
}
