//! TOFU-approved client set, persisted in app NVM.
//!
//! The Ledger port of the Heartwood approval model (`heartwood-common`
//! `policy.rs`): non-signing methods are safe once a client can speak the
//! NIP-44 conversation (the `CONNECT_SAFE_METHODS` tier); `sign_event`
//! requires one physical approval, after which the client holds the
//! `TOFU_SAFE_METHODS` grant — auto-approved signing, the unattended-bunker
//! model — until revoked. Only the approved pubkey set needs persisting: the
//! grant it encodes is exactly `make_tofu_policy` (empty kind allowlist =
//! all kinds), so the policies rebuild from pubkeys alone.
//!
//! Storage is a fixed array in the app's NVM (no filesystem on BOLOS);
//! `AtomicStorage` double-buffers it against tearing. When full, the oldest
//! entry is dropped (FIFO), matching `MAX_AUTHORIZED_PUBKEYS` behaviour.

use ledger_device_sdk::nvm::{AtomicStorage, SingleStorage};
use ledger_device_sdk::NVMData;

pub const MAX_CLIENTS: usize = 8;

#[derive(Copy, Clone)]
pub struct ApprovedClients {
    count: u8,
    keys: [[u8; 32]; MAX_CLIENTS],
}

const EMPTY: ApprovedClients = ApprovedClients {
    count: 0,
    keys: [[0u8; 32]; MAX_CLIENTS],
};

#[link_section = ".nvm_data"]
static mut APPROVED: NVMData<AtomicStorage<ApprovedClients>> =
    NVMData::new(AtomicStorage::new(&EMPTY));

fn storage() -> &'static mut AtomicStorage<ApprovedClients> {
    // SAFETY: single-threaded app; NVMData handles PIC relocation.
    unsafe {
        let p = &raw mut APPROVED;
        (*p).get_mut()
    }
}

pub fn is_approved(client_pk: &[u8; 32]) -> bool {
    let s = storage().get_ref();
    s.keys[..s.count as usize].iter().any(|k| k == client_pk)
}

/// Record a physical approval (idempotent; FIFO-evicts the oldest when full).
pub fn approve(client_pk: &[u8; 32]) {
    if is_approved(client_pk) {
        return;
    }
    let mut next = *storage().get_ref();
    if (next.count as usize) < MAX_CLIENTS {
        next.keys[next.count as usize] = *client_pk;
        next.count += 1;
    } else {
        next.keys.copy_within(1.., 0);
        next.keys[MAX_CLIENTS - 1] = *client_pk;
    }
    storage().update(&next);
}
