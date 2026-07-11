//! Minimal NBGL home screen. PoC: no settings, no per-request review screens —
//! see the `sign_path` module note on what must land before review.

use ledger_device_sdk::include_gif;
use ledger_device_sdk::io::Comm;
use ledger_device_sdk::nbgl::{NbglGlyph, NbglHomeAndSettings};

pub fn menu_main(_: &mut Comm) -> NbglHomeAndSettings {
    const GLYPH: NbglGlyph =
        NbglGlyph::from_include(include_gif!("glyphs/home_nano_nbgl.png", NBGL));

    NbglHomeAndSettings::new()
        .glyph(&GLYPH)
        .infos("Heartwood", env!("CARGO_PKG_VERSION"), "ForgeSworn")
}
