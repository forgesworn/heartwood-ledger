//! NBGL screens: the home menu and the TOFU signing-approval prompt.

use alloc::format;

use ledger_device_sdk::include_gif;
use ledger_device_sdk::io::Comm;
use ledger_device_sdk::nbgl::{NbglChoice, NbglGlyph, NbglHomeAndSettings};

use heartwood_common::encoding::encode_npub;

pub fn menu_main(_: &mut Comm) -> NbglHomeAndSettings {
    const GLYPH: NbglGlyph =
        NbglGlyph::from_include(include_gif!("glyphs/home_nano_nbgl.png", NBGL));

    NbglHomeAndSettings::new()
        .glyph(&GLYPH)
        .infos("Heartwood", env!("CARGO_PKG_VERSION"), "ForgeSworn")
}

/// The physical-approval gate for `sign_event`. Returns immediately for a
/// TOFU-approved client; otherwise blocks on an on-device choice screen and,
/// on approval, records the client in NVM (see `approvals`). The prompt shows
/// the client npub prefix and the event kind — enough to recognise a pairing,
/// small enough for the nano screen.
pub fn approve_signing<const N: usize>(
    comm: &mut Comm<N>,
    client_pk: &[u8; 32],
    kind: u64,
) -> bool {
    if crate::approvals::is_approved(client_pk) {
        return true;
    }
    let npub = encode_npub(client_pk);
    let sub = format!("{}... kind {}", &npub[..14], kind);
    let ok = NbglChoice::new().show(comm, "Approve signer?", &sub, "Approve", "Reject");
    if ok {
        crate::approvals::approve(client_pk);
    }
    ok
}
