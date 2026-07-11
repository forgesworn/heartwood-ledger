//! Interop proof driver. Run against the app in Speculos seeded with the
//! canonical all-zero BIP-39 phrase:
//!
//!   speculos --model nanosp --display headless \
//!     --seed "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about" \
//!     target/nanosplus/release/heartwood-ledger
//!
//! Then: cargo run  (in host/)
//!
//! What it proves, in order:
//!   1. The Ledger's BIP-32 node at m/44'/1237'/727'/0'/0' equals the frozen
//!      tree-root vector shared with the provision CLI / sapwood / firmware —
//!      i.e. a heartwood phrase restored on a Ledger IS the same identity.
//!   2. A full NIP-46 `get_public_key` round trip: NIP-44 encrypt on host,
//!      decrypt+dispatch+re-encrypt+sign on device, envelope BIP-340 signature
//!      verifies, response decrypts to the master pubkey.
//!   3. `sign_event` as master: inner event id + BIP-340 signature verify.
//!   4. nsec-tree personas: `heartwood_derive_persona` + `heartwood_switch` +
//!      `sign_event` signs as the persona, whose key matches host-side
//!      heartwood-common derivation byte-for-byte.

use std::io::{Read, Write};
use std::net::TcpStream;

use heartwood_common::derive::{create_tree_root, derive};
use heartwood_common::encoding::encode_npub;
use heartwood_common::hex::{hex_decode, hex_encode};
use heartwood_common::nip44;
use heartwood_common::nip46::{self, UnsignedEvent};
use k256::schnorr::signature::hazmat::PrehashVerifier;
use k256::schnorr::{Signature, SigningKey, VerifyingKey};

/// The canonical all-zero BIP-39 vector's tree root (see
/// heartwood-esp32 common/src/mnemonic.rs and the provision CLI tests).
const ZERO_ROOT_HEX: &str = "cc92d213b5eccd19eb85c12c2cf6fd168f27c2cc347c51a7c4c62ac67795fc65";
/// A fixed, valid client secret for the NIP-46 conversation.
const CLIENT_SECRET: [u8; 32] = [7u8; 32];
const CREATED_AT: u64 = 1_752_000_000;
const CHUNK: usize = 250;

struct Speculos(TcpStream);

impl Speculos {
    fn connect() -> Self {
        let addr = std::env::var("SPECULOS_ADDR").unwrap_or_else(|_| "127.0.0.1:9999".into());
        Speculos(TcpStream::connect(&addr).unwrap_or_else(|e| panic!("connect {addr}: {e}")))
    }

    /// Length-prefixed APDU exchange (ledgercomm TCP framing):
    /// send u32-be length + APDU; receive u32-be data length, data, then 2-byte SW.
    fn apdu(&mut self, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> (Vec<u8>, u16) {
        assert!(data.len() <= 255, "APDU data too long");
        let mut apdu = vec![cla, ins, p1, p2, data.len() as u8];
        apdu.extend_from_slice(data);
        let mut msg = (apdu.len() as u32).to_be_bytes().to_vec();
        msg.extend_from_slice(&apdu);
        self.0.write_all(&msg).expect("apdu write");

        let mut len_buf = [0u8; 4];
        self.0.read_exact(&mut len_buf).expect("apdu read len");
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0u8; len];
        self.0.read_exact(&mut payload).expect("apdu read data");
        let mut sw = [0u8; 2];
        self.0.read_exact(&mut sw).expect("apdu read sw");
        (payload, u16::from_be_bytes(sw))
    }

    fn expect_ok(&mut self, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Vec<u8> {
        let (payload, sw) = self.apdu(cla, ins, p1, p2, data);
        assert_eq!(sw, 0x9000, "APDU ins={ins:#04x} failed with SW {sw:#06x}");
        payload
    }

    /// Send a 0x10 frame body in ≤250-byte chunks; collect the 0x35 JSON via 0x11.
    fn process(&mut self, payload: &[u8]) -> String {
        let chunks: Vec<&[u8]> = payload.chunks(CHUNK).collect();
        let mut total = 0u16;
        for (i, chunk) in chunks.iter().enumerate() {
            let more = if i + 1 == chunks.len() { 0x00 } else { 0x80 };
            let resp = self.expect_ok(0xe0, 0x10, i as u8, more, chunk);
            if more == 0x00 {
                total = u16::from_be_bytes(resp[..2].try_into().unwrap());
            }
        }
        let mut out = Vec::with_capacity(total as usize);
        let mut chunk_idx = 0u8;
        while out.len() < total as usize {
            let part = self.expect_ok(0xe0, 0x11, chunk_idx, 0, &[]);
            assert!(!part.is_empty(), "GET_RESULT returned no data");
            out.extend_from_slice(&part);
            chunk_idx += 1;
        }
        String::from_utf8(out).expect("result is UTF-8")
    }
}

/// Encrypt a NIP-46 request JSON to the device and wrap it in the 0x10 body:
/// `[master_pk 32][client_pk 32][created_at u64-be 8][nip44_ciphertext_b64]`.
fn build_request(master_pk: &[u8; 32], client_pk: &[u8; 32], ck: &[u8; 32], json: &str) -> Vec<u8> {
    let nonce = nip44::synthetic_nonce(&CLIENT_SECRET, master_pk, json);
    let ct = nip44::encrypt(ck, json, &nonce).expect("nip44 encrypt");
    let mut payload = Vec::with_capacity(72 + ct.len());
    payload.extend_from_slice(master_pk);
    payload.extend_from_slice(client_pk);
    payload.extend_from_slice(&CREATED_AT.to_be_bytes());
    payload.extend_from_slice(ct.as_bytes());
    payload
}

/// Verify the kind:24133 envelope (id + BIP-340 sig under the master key) and
/// return the decrypted NIP-46 response JSON.
fn open_envelope(envelope_json: &str, master_pk: &[u8; 32], ck: &[u8; 32]) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(envelope_json).expect("envelope JSON");
    assert_eq!(v["kind"].as_u64(), Some(24133), "envelope kind");
    assert_eq!(v["pubkey"].as_str(), Some(hex_encode(master_pk).as_str()), "envelope author");

    let unsigned = UnsignedEvent {
        pubkey: v["pubkey"].as_str().unwrap().to_string(),
        created_at: v["created_at"].as_u64().unwrap(),
        kind: v["kind"].as_u64().unwrap(),
        tags: serde_json::from_value(v["tags"].clone()).unwrap(),
        content: v["content"].as_str().unwrap().to_string(),
    };
    let id = nip46::compute_event_id(&unsigned);
    assert_eq!(hex_encode(&id), v["id"].as_str().unwrap(), "envelope id");

    verify_bip340(master_pk, &id, v["sig"].as_str().unwrap());

    let response = nip44::decrypt(ck, unsigned.content.as_str()).expect("nip44 decrypt response");
    serde_json::from_str(&response).expect("response JSON")
}

fn verify_bip340(pk: &[u8; 32], msg: &[u8; 32], sig_hex: &str) {
    let vk = VerifyingKey::from_bytes(pk).expect("verifying key");
    let sig_bytes = hex_decode(sig_hex).expect("sig hex");
    let sig = Signature::try_from(sig_bytes.as_slice()).expect("sig parse");
    vk.verify_prehash(msg, &sig).expect("BIP-340 signature invalid");
}

fn main() {
    let mut dev = Speculos::connect();

    // --- 0. App identity sanity ---------------------------------------------
    let name = dev.expect_ok(0xe0, 0x04, 0, 0, &[]);
    assert_eq!(name, b"Heartwood", "app name");

    // --- 1. Derivation interop: frozen vector -------------------------------
    let root_bytes = hex_decode(ZERO_ROOT_HEX).unwrap();
    let root: [u8; 32] = root_bytes.as_slice().try_into().unwrap();
    let expected_pk: [u8; 32] = SigningKey::from_bytes(&root)
        .unwrap()
        .verifying_key()
        .to_bytes()
        .into();

    let device_pk_raw = dev.expect_ok(0xe0, 0x05, 0, 0, &[]);
    let device_pk: [u8; 32] = device_pk_raw.as_slice().try_into().expect("32-byte pubkey");
    assert_eq!(
        device_pk, expected_pk,
        "device master pubkey does not match the frozen all-zero vector"
    );
    println!("PASS 1: derivation interop — device npub {}", encode_npub(&device_pk));

    // Conversation key for everything that follows.
    let client_pk: [u8; 32] = SigningKey::from_bytes(&CLIENT_SECRET)
        .unwrap()
        .verifying_key()
        .to_bytes()
        .into();
    let ck = nip44::get_conversation_key(&CLIENT_SECRET, &device_pk).expect("conversation key");

    // --- 2. NIP-46 get_public_key round trip --------------------------------
    let req = r#"{"id":"t1","method":"get_public_key","params":[]}"#;
    let envelope = dev.process(&build_request(&device_pk, &client_pk, &ck, req));
    let resp = open_envelope(&envelope, &device_pk, &ck);
    assert_eq!(resp["id"].as_str(), Some("t1"));
    assert_eq!(resp["result"].as_str(), Some(hex_encode(&device_pk).as_str()));
    println!("PASS 2: NIP-46 get_public_key round trip (envelope sig + NIP-44 both verified)");

    // --- 3. sign_event as master --------------------------------------------
    let inner = serde_json::json!({
        "kind": 1,
        "created_at": CREATED_AT,
        "tags": [],
        "content": "hello from a Ledger running heartwood",
        "pubkey": hex_encode(&device_pk),
    });
    let req = serde_json::json!({
        "id": "t2",
        "method": "sign_event",
        "params": [inner.to_string()],
    });
    let envelope = dev.process(&build_request(&device_pk, &client_pk, &ck, &req.to_string()));
    let resp = open_envelope(&envelope, &device_pk, &ck);
    let signed: serde_json::Value =
        serde_json::from_str(resp["result"].as_str().expect("sign result")).unwrap();
    let ev = UnsignedEvent {
        pubkey: signed["pubkey"].as_str().unwrap().to_string(),
        created_at: signed["created_at"].as_u64().unwrap(),
        kind: signed["kind"].as_u64().unwrap(),
        tags: serde_json::from_value(signed["tags"].clone()).unwrap(),
        content: signed["content"].as_str().unwrap().to_string(),
    };
    assert_eq!(ev.pubkey, hex_encode(&device_pk), "inner author is master");
    let id = nip46::compute_event_id(&ev);
    assert_eq!(hex_encode(&id), signed["id"].as_str().unwrap(), "inner id");
    verify_bip340(&device_pk, &id, signed["sig"].as_str().unwrap());
    println!("PASS 3: sign_event as master — inner id + BIP-340 sig verified");

    // --- 4. nsec-tree persona: derive, switch, sign --------------------------
    let tree = create_tree_root(&root).unwrap();
    let expected_persona = derive(&tree, "nostr:persona:alice", 0).unwrap();

    let req = r#"{"id":"t3","method":"heartwood_derive_persona","params":["alice"]}"#;
    let envelope = dev.process(&build_request(&device_pk, &client_pk, &ck, req));
    let resp = open_envelope(&envelope, &device_pk, &ck);
    let derived: serde_json::Value =
        serde_json::from_str(resp["result"].as_str().expect("derive result")).unwrap();
    assert_eq!(derived["npub"].as_str(), Some(expected_persona.npub.as_str()),
        "persona npub does not match host-side heartwood-common derivation");

    let req = r#"{"id":"t4","method":"heartwood_switch","params":["alice"]}"#;
    let envelope = dev.process(&build_request(&device_pk, &client_pk, &ck, req));
    let resp = open_envelope(&envelope, &device_pk, &ck);
    assert!(resp["result"].as_str().is_some(), "switch failed: {resp}");

    let inner = serde_json::json!({
        "kind": 1,
        "created_at": CREATED_AT,
        "tags": [],
        "content": "hello from alice, an nsec-tree persona on a Ledger",
        "pubkey": "",
    });
    let req = serde_json::json!({
        "id": "t5",
        "method": "sign_event",
        "params": [inner.to_string()],
    });
    let envelope = dev.process(&build_request(&device_pk, &client_pk, &ck, &req.to_string()));
    let resp = open_envelope(&envelope, &device_pk, &ck);
    let signed: serde_json::Value =
        serde_json::from_str(resp["result"].as_str().expect("persona sign result")).unwrap();
    assert_eq!(
        signed["pubkey"].as_str(),
        Some(hex_encode(&expected_persona.public_key).as_str()),
        "persona event author"
    );
    let ev = UnsignedEvent {
        pubkey: signed["pubkey"].as_str().unwrap().to_string(),
        created_at: signed["created_at"].as_u64().unwrap(),
        kind: signed["kind"].as_u64().unwrap(),
        tags: serde_json::from_value(signed["tags"].clone()).unwrap(),
        content: signed["content"].as_str().unwrap().to_string(),
    };
    let id = nip46::compute_event_id(&ev);
    assert_eq!(hex_encode(&id), signed["id"].as_str().unwrap(), "persona inner id");
    verify_bip340(&expected_persona.public_key, &id, signed["sig"].as_str().unwrap());
    println!("PASS 4: persona derive/switch/sign — matches host derivation, sig verified");

    println!("\nALL PASS — heartwood identity + NIP-46 + NIP-44 + nsec-tree proven on Ledger (Speculos)");
}
