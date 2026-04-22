use alloc::{string::String, vec::Vec};
use core::convert::TryInto;

use codama_parser::{parse_program_index, DecodedInstruction, ProgramIndex};
use ledger_device_sdk::{
    ecc::{CurvesId, ECPublicKey},
    nvm::*,
    NVMData,
};
use ledger_secure_sdk_sys::{
    cx_edwards_decompress_point_no_throw, CX_CURVE_Ed25519, CX_OK, CX_SHA512,
};

use crate::AppSW;

pub const IDL_IMPORT_PAYLOAD_VERSION: u8 = 1;
pub const IDL_ATTESTATION_DOMAIN_SEPARATOR: &[u8] = b"ledger-solana-idl-attestation-v1:";
pub const MAX_IMPORTED_IDL_BYTES: usize = 2048;
pub const MAX_IDL_ATTESTATIONS: usize = 4;

const IMPORTED_IDL_STORAGE_VERSION: u8 = 1;
const IMPORTED_IDL_HEADER_SIZE: usize = 1 + 2 + 32;
const IMPORTED_IDL_STORAGE_SIZE: usize = IMPORTED_IDL_HEADER_SIZE + MAX_IMPORTED_IDL_BYTES;
const IDL_ATTESTATION_SIZE: usize = 32 + 64;

#[link_section = ".nvm_data"]
static mut IMPORTED_IDL_DATA: NVMData<AtomicStorage<[u8; IMPORTED_IDL_STORAGE_SIZE]>> =
    NVMData::new(AtomicStorage::new(&[0u8; IMPORTED_IDL_STORAGE_SIZE]));

pub struct LoadedIdl {
    pub name: String,
    pub program_id: [u8; 32],
    pub source: LoadedIdlSource,
    program: ProgramIndex<'static>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LoadedIdlSource {
    Builtin,
    Imported,
}

pub struct ResolvedDecodedInstruction {
    pub program_name: String,
    pub instruction: DecodedInstruction,
}

pub struct LoadIdlResponse {
    pub program_id: [u8; 32],
    pub signer_count: u8,
    pub idl_len: u16,
}

pub struct PreparedIdlImport<'a> {
    parsed: ParsedIdlImportPayload<'a>,
    pub program_id: [u8; 32],
    pub signer_pubkeys: Vec<[u8; 32]>,
}

struct BuiltinIdlSource {
    name: &'static str,
    bytes: &'static [u8],
}

struct ParsedIdlImportPayload<'a> {
    signer_count: u8,
    attestation_bytes: &'a [u8],
    idl_bytes: &'a [u8],
    idl_len: u16,
}

const BUILTIN_IDLS: &[BuiltinIdlSource] = &[
    BuiltinIdlSource {
        name: "system",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idls/system.codama.json"
        )),
    },
    BuiltinIdlSource {
        name: "compute-budget",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idls/compute-budget.codama.json"
        )),
    },
    BuiltinIdlSource {
        name: "associated-token-account",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idls/associated-token-account.codama.json"
        )),
    },
    BuiltinIdlSource {
        name: "token",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idls/token.codama.json"
        )),
    },
];

pub fn load_idls() -> Vec<LoadedIdl> {
    let mut loaded = Vec::with_capacity(BUILTIN_IDLS.len() + 1);
    for builtin in BUILTIN_IDLS {
        if let Some(loaded_idl) = load_builtin_idl(builtin) {
            loaded.push(loaded_idl);
        }
    }
    if let Some(imported) = load_imported_idl() {
        loaded.push(imported);
    }
    loaded
}

pub fn decode_instruction(
    loaded_idls: &[LoadedIdl],
    program_id: &[u8; 32],
    instruction_data: &[u8],
) -> Option<ResolvedDecodedInstruction> {
    let loaded_idl = loaded_idls
        .iter()
        .find(|loaded_idl| &loaded_idl.program_id == program_id)?;
    let instruction = loaded_idl
        .program
        .decode_instruction_data(instruction_data)
        .ok()?;

    Some(ResolvedDecodedInstruction {
        program_name: loaded_idl.name.clone(),
        instruction,
    })
}

pub fn prepare_idl_import(payload: &[u8]) -> Result<PreparedIdlImport<'_>, AppSW> {
    let parsed = parse_import_payload(payload)?;
    let program = parse_program_index(parsed.idl_bytes).map_err(|_| AppSW::InvalidData)?;
    let program_id = decode_program_id(program.public_key.as_str()).ok_or(AppSW::InvalidData)?;
    Ok(PreparedIdlImport {
        program_id,
        signer_pubkeys: collect_signer_pubkeys(parsed.attestation_bytes)?,
        parsed,
    })
}

pub fn verify_prepared_idl_import(prepared: &PreparedIdlImport<'_>) -> Result<(), AppSW> {
    verify_attestations(&prepared.parsed)
}

pub fn store_prepared_idl(prepared: &PreparedIdlImport<'_>) -> LoadIdlResponse {
    store_imported_idl(prepared.parsed.idl_bytes, &prepared.program_id);
    LoadIdlResponse {
        program_id: prepared.program_id,
        signer_count: prepared.parsed.signer_count,
        idl_len: prepared.parsed.idl_len,
    }
}

pub fn decode_program_id(program_id: &str) -> Option<[u8; 32]> {
    let bytes = bs58::decode(program_id).into_vec().ok()?;
    if bytes.len() != 32 {
        return None;
    }

    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
}

fn load_builtin_idl(builtin: &'static BuiltinIdlSource) -> Option<LoadedIdl> {
    let program = parse_program_index(builtin.bytes).ok()?;
    let program_id = decode_program_id(&program.public_key)?;
    Some(LoadedIdl {
        name: String::from(builtin.name),
        program_id,
        source: LoadedIdlSource::Builtin,
        program,
    })
}

fn load_imported_idl() -> Option<LoadedIdl> {
    let data = &raw const IMPORTED_IDL_DATA;
    let storage = unsafe { (*data).get_ref() };
    let stored = storage.get_ref();
    if stored[0] != IMPORTED_IDL_STORAGE_VERSION {
        return None;
    }

    let idl_len = u16::from_be_bytes([stored[1], stored[2]]) as usize;
    if idl_len == 0 || idl_len > MAX_IMPORTED_IDL_BYTES {
        return None;
    }

    let mut program_id = [0u8; 32];
    program_id.copy_from_slice(&stored[3..35]);
    let idl_bytes = &stored[IMPORTED_IDL_HEADER_SIZE..IMPORTED_IDL_HEADER_SIZE + idl_len];
    let program = parse_program_index(idl_bytes).ok()?;
    Some(LoadedIdl {
        name: program.name.clone(),
        program_id,
        source: LoadedIdlSource::Imported,
        program,
    })
}

fn parse_import_payload(payload: &[u8]) -> Result<ParsedIdlImportPayload<'_>, AppSW> {
    if payload.len() < 4 || payload[0] != IDL_IMPORT_PAYLOAD_VERSION {
        return Err(AppSW::InvalidData);
    }

    let signer_count = payload[1] as usize;
    if signer_count == 0 || signer_count > MAX_IDL_ATTESTATIONS {
        return Err(AppSW::InvalidData);
    }

    let idl_len = u16::from_be_bytes([payload[2], payload[3]]) as usize;
    if idl_len == 0 || idl_len > MAX_IMPORTED_IDL_BYTES {
        return Err(AppSW::WrongApduLength);
    }

    let attestation_len = signer_count
        .checked_mul(IDL_ATTESTATION_SIZE)
        .ok_or(AppSW::WrongApduLength)?;
    let expected_len = 4usize
        .checked_add(attestation_len)
        .and_then(|value| value.checked_add(idl_len))
        .ok_or(AppSW::WrongApduLength)?;
    if payload.len() != expected_len {
        return Err(AppSW::InvalidData);
    }

    let attestation_bytes = &payload[4..4 + attestation_len];
    let idl_bytes = &payload[4 + attestation_len..];
    Ok(ParsedIdlImportPayload {
        signer_count: signer_count as u8,
        attestation_bytes,
        idl_bytes,
        idl_len: idl_len as u16,
    })
}

fn verify_attestations(parsed: &ParsedIdlImportPayload<'_>) -> Result<(), AppSW> {
    let attestation_message = attestation_message(parsed.idl_bytes);
    for chunk in parsed.attestation_bytes.chunks_exact(IDL_ATTESTATION_SIZE) {
        let signer_pubkey: &[u8; 32] = chunk[..32].try_into().map_err(|_| AppSW::InvalidData)?;
        let signature: &[u8; 64] = chunk[32..].try_into().map_err(|_| AppSW::InvalidData)?;
        if !verify_ed25519_signature(signer_pubkey, signature, attestation_message.as_slice())? {
            return Err(AppSW::InvalidData);
        }
    }
    Ok(())
}

fn collect_signer_pubkeys(attestation_bytes: &[u8]) -> Result<Vec<[u8; 32]>, AppSW> {
    let mut signer_pubkeys = Vec::with_capacity(attestation_bytes.len() / IDL_ATTESTATION_SIZE);
    for chunk in attestation_bytes.chunks_exact(IDL_ATTESTATION_SIZE) {
        let mut signer_pubkey = [0u8; 32];
        signer_pubkey.copy_from_slice(&chunk[..32]);
        signer_pubkeys.push(signer_pubkey);
    }
    Ok(signer_pubkeys)
}

fn attestation_message(idl_bytes: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(IDL_ATTESTATION_DOMAIN_SEPARATOR.len() + idl_bytes.len());
    message.extend_from_slice(IDL_ATTESTATION_DOMAIN_SEPARATOR);
    message.extend_from_slice(idl_bytes);
    message
}

fn verify_ed25519_signature(
    signer_pubkey: &[u8; 32],
    signature: &[u8; 64],
    message: &[u8],
) -> Result<bool, AppSW> {
    let mut public_key = ECPublicKey::<65, 'E'>::new(CurvesId::Ed25519);
    public_key.pubkey[1..33].copy_from_slice(signer_pubkey);
    let err = unsafe {
        cx_edwards_decompress_point_no_throw(
            CX_CURVE_Ed25519,
            public_key.pubkey.as_mut_ptr(),
            public_key.keylength,
        )
    };
    if err != CX_OK {
        return Err(AppSW::InvalidData);
    }

    Ok(public_key.verify(
        (signature.as_slice(), signature.len() as u32),
        message,
        CX_SHA512,
    ))
}

fn store_imported_idl(idl_bytes: &[u8], program_id: &[u8; 32]) {
    let mut updated = [0u8; IMPORTED_IDL_STORAGE_SIZE];
    updated[0] = IMPORTED_IDL_STORAGE_VERSION;
    updated[1..3].copy_from_slice(&(idl_bytes.len() as u16).to_be_bytes());
    updated[3..35].copy_from_slice(program_id);
    updated[IMPORTED_IDL_HEADER_SIZE..IMPORTED_IDL_HEADER_SIZE + idl_bytes.len()]
        .copy_from_slice(idl_bytes);

    let data = &raw mut IMPORTED_IDL_DATA;
    let storage = unsafe { (*data).get_mut() };
    storage.update(&updated);
}

pub fn clear_imported_idl() {
    let cleared = [0u8; IMPORTED_IDL_STORAGE_SIZE];
    let data = &raw mut IMPORTED_IDL_DATA;
    let storage = unsafe { (*data).get_mut() };
    storage.update(&cleared);
}
