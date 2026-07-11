//! Heartwood Nostr signer as a Ledger embedded app.
//!
//! The device half of the daemon-mediated signer, on Ledger hardware: the host
//! (heartwood-bridge, or the host driver in `host/`) delivers the body of an
//! `ENCRYPTED_REQUEST` (serial frame 0x10) over chunked APDUs; the app does the
//! NIP-44 decrypt → NIP-46 dispatch → re-encrypt → kind:24133 envelope signing
//! inline and hands back the `SIGN_ENVELOPE_RESPONSE` (0x35) JSON over chunked
//! APDUs. Key material derives from the Ledger's own seed at the heartwood path
//! `m/44'/1237'/727'/0'/0'` and never leaves the device.
//!
//! APDU protocol (CLA 0xE0):
//!   INS 0x03 GET_VERSION                       → [major, minor, patch]
//!   INS 0x04 GET_APP_NAME                      → b"Heartwood"
//!   INS 0x05 GET_PUBKEY                        → 32-byte x-only master pubkey
//!   INS 0x10 PROCESS  p1=chunk#, p2=0x80 more / 0x00 last, data=payload chunk
//!            (accumulates the 0x10 frame body; on the last chunk, dispatches
//!             and replies with the result length as a u16 big-endian)
//!   INS 0x11 GET_RESULT p1=chunk#              → next ≤250-byte slice of result

#![no_std]
#![no_main]

mod crypto;
mod identity;
mod seed;
mod sign_path;
mod ui;

extern crate alloc;

use alloc::vec::Vec;

use heartwood_common::types::MasterMode;
use identity::{IdentityCache, Sessions};
use ledger_device_sdk::io::{self, init_comm, ApduHeader, Command, Reply, StatusWords};

ledger_device_sdk::set_panic!(ledger_device_sdk::exiting_panic);
ledger_device_sdk::define_comm!(COMM);

/// Per-APDU payload chunk ceiling (kept under the 255-byte APDU data limit).
const CHUNK: usize = 250;
/// Accumulated-request ceiling — protects the 24 KB heap from a runaway host.
const MAX_REQUEST: usize = 8 * 1024;

// Application status words.
#[repr(u16)]
#[derive(Clone, Copy, PartialEq)]
pub enum AppSW {
    Deny = 0x6985,
    WrongP1P2 = 0x6A86,
    InsNotSupported = 0x6D00,
    CommError = 0x6F00,
    KeyDeriveFail = 0xB009,
    RequestTooLong = 0xB010,
    ChunkOutOfOrder = 0xB011,
    ProcessFail = 0xB012,
    NoResult = 0xB013,
    WrongApduLength = StatusWords::BadLen as u16,
    Ok = 0x9000,
}

impl From<AppSW> for Reply {
    fn from(sw: AppSW) -> Reply {
        Reply(sw as u16)
    }
}

impl From<io::CommError> for AppSW {
    fn from(_e: io::CommError) -> Self {
        AppSW::CommError
    }
}

/// Possible input commands received through APDUs.
#[derive(Debug)]
pub enum Instruction {
    GetVersion,
    GetAppName,
    GetPubkey,
    Process { chunk: u8, more: bool },
    GetResult { chunk: u8 },
}

impl TryFrom<ApduHeader> for Instruction {
    type Error = AppSW;

    fn try_from(value: ApduHeader) -> Result<Self, Self::Error> {
        match (value.ins, value.p1, value.p2) {
            (3, 0, 0) => Ok(Instruction::GetVersion),
            (4, 0, 0) => Ok(Instruction::GetAppName),
            (5, 0, 0) => Ok(Instruction::GetPubkey),
            (0x10, chunk, 0x00 | 0x80) => Ok(Instruction::Process {
                chunk,
                more: value.p2 == 0x80,
            }),
            (0x11, chunk, 0) => Ok(Instruction::GetResult { chunk }),
            (3..=5 | 0x10 | 0x11, _, _) => Err(AppSW::WrongP1P2),
            (_, _, _) => Err(AppSW::InsNotSupported),
        }
    }
}

/// RAM-only session state, reset when the app is closed.
struct AppState {
    /// Accumulating 0x10 frame body across Process chunks.
    buf: Vec<u8>,
    /// Next expected Process chunk index.
    next_chunk: u8,
    /// The signed kind:24133 envelope JSON awaiting GetResult collection.
    result: Option<Vec<u8>>,
    cache: IdentityCache,
    sessions: Sessions,
}

impl AppState {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            next_chunk: 0,
            result: None,
            cache: IdentityCache::new(),
            sessions: Sessions::new(),
        }
    }
}

#[no_mangle]
extern "C" fn sample_main() {
    let comm = init_comm(&COMM);
    comm.set_expected_cla(0xe0);

    let mut state = AppState::new();
    let mut home = ui::menu_main(comm);
    home.show_and_return();

    loop {
        let command = comm.next_command();
        let decoded = command.decode::<Instruction>();
        let Ok(ins) = decoded else {
            let _ = comm.send(&[], decoded.unwrap_err());
            continue;
        };

        match handle_apdu(command, &ins, &mut state) {
            Ok(reply) => {
                let _ = reply.send(AppSW::Ok);
            }
            Err(sw) => {
                let _ = comm.send(&[], sw);
            }
        }
    }
}

fn handle_apdu<'a>(
    command: Command<'a>,
    ins: &Instruction,
    state: &mut AppState,
) -> Result<io::CommandResponse<'a>, AppSW> {
    match ins {
        Instruction::GetAppName => {
            let mut response = command.into_response();
            response.append(b"Heartwood")?;
            Ok(response)
        }
        Instruction::GetVersion => {
            let (major, minor, patch) = version_triplet();
            let mut response = command.into_response();
            response.append(&[major, minor, patch])?;
            Ok(response)
        }
        Instruction::GetPubkey => {
            let master = seed::master_secret().ok_or(AppSW::KeyDeriveFail)?;
            let pk = crypto::pubkey(&master).ok_or(AppSW::KeyDeriveFail)?;
            let mut response = command.into_response();
            response.append(&pk)?;
            Ok(response)
        }
        Instruction::Process { chunk, more } => {
            if *chunk == 0 {
                state.buf.clear();
                state.next_chunk = 0;
                state.result = None;
            }
            if *chunk != state.next_chunk {
                state.buf.clear();
                state.next_chunk = 0;
                return Err(AppSW::ChunkOutOfOrder);
            }
            let data = command.get_data();
            if state.buf.len() + data.len() > MAX_REQUEST {
                state.buf.clear();
                state.next_chunk = 0;
                return Err(AppSW::RequestTooLong);
            }
            state.buf.extend_from_slice(data);
            state.next_chunk = state.next_chunk.wrapping_add(1);

            if *more {
                return Ok(command.into_response());
            }

            // Last chunk: derive, dispatch, stash the signed envelope.
            let master = seed::master_secret().ok_or(AppSW::KeyDeriveFail)?;
            let json = sign_path::handle(
                &master,
                MasterMode::TreeMnemonic as u8,
                &state.buf,
                &mut state.cache,
                &mut state.sessions,
            )
            .ok_or(AppSW::ProcessFail)?;
            state.buf.clear();
            state.next_chunk = 0;

            let bytes = json.into_bytes();
            let len = (bytes.len() as u16).to_be_bytes();
            state.result = Some(bytes);

            let mut response = command.into_response();
            response.append(&len)?;
            Ok(response)
        }
        Instruction::GetResult { chunk } => {
            let result = state.result.as_ref().ok_or(AppSW::NoResult)?;
            let start = *chunk as usize * CHUNK;
            if start >= result.len() {
                return Err(AppSW::NoResult);
            }
            let end = core::cmp::min(start + CHUNK, result.len());
            let mut response = command.into_response();
            response.append(&result[start..end])?;
            Ok(response)
        }
    }
}

/// `CARGO_PKG_VERSION` as bytes, without pulling in a parser.
fn version_triplet() -> (u8, u8, u8) {
    let mut parts = [0u8; 3];
    let mut i = 0;
    for b in env!("CARGO_PKG_VERSION").bytes() {
        match b {
            b'.' => i += 1,
            b'0'..=b'9' if i < 3 => parts[i] = parts[i].wrapping_mul(10) + (b - b'0'),
            _ => break,
        }
    }
    (parts[0], parts[1], parts[2])
}
