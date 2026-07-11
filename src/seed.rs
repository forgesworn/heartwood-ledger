//! The device master secret: the BIP-32 node at `m/44'/1237'/727'/0'/0'`.
//!
//! This is the exact path `heartwood_common::mnemonic::derive_root_secret` walks
//! from a BIP-39 phrase, so a phrase restored onto the Ledger through its normal
//! onboarding yields the same tree-root secret — and therefore the same npub and
//! nsec-tree personas — as an ESP32/ESP8266 signer or the provision CLI given
//! that phrase. Verified against the shared all-zero vector by the host driver.
//!
//! The OS only hands out nodes under the `44'/1237'` prefix declared in the app
//! manifest; the raw seed itself is never accessible.

use ledger_device_sdk::ecc::{bip32_derive, CurvesId, Secret};
use zeroize::Zeroizing;

const HARDENED: u32 = 0x8000_0000;
/// Must match `heartwood_common::types::MNEMONIC_PATH` (`m/44'/1237'/727'/0'/0'`).
const PATH: [u32; 5] = [
    44 | HARDENED,
    1237 | HARDENED,
    727 | HARDENED,
    HARDENED,
    HARDENED,
];

/// Derive the 32-byte tree-root secret from the device seed. The 64-byte node
/// buffer (key || chain code scratch) is zeroised on drop.
pub fn master_secret() -> Option<Zeroizing<[u8; 32]>> {
    let mut node: Secret<64> = Secret::new();
    bip32_derive(CurvesId::Secp256k1, &PATH, node.as_mut(), None).ok()?;
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&node.as_ref()[..32]);
    Some(out)
}
