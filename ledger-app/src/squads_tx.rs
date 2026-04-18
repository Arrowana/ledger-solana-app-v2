use arrayvec::ArrayVec;
use ledger_device_sdk::ecc::Ed25519;
use ledger_device_sdk::hash::{sha2::Sha2_256, HashInit};
use ledger_secure_sdk_sys::{
    cx_decode_coord, cx_ecpoint_alloc, cx_ecpoint_decompress, cx_ecpoint_is_on_curve, cx_ecpoint_t,
    CX_CURVE_Ed25519, CX_OK,
};

use crate::storage::{MAX_DERIVATION_PATH_LENGTH, PUBKEY_LENGTH};
use crate::AppSW;

pub const SIGNATURE_LENGTH: usize = 64;
pub const BLOCKHASH_LENGTH: usize = 32;
pub const MESSAGE_HASH_LENGTH: usize = 32;
pub const MAX_MESSAGE_LENGTH: usize = 512;
pub const PROPOSAL_VOTE_APPROVE: u8 = 0x00;
pub const PROPOSAL_VOTE_REJECT: u8 = 0x01;

const SYSTEM_PROGRAM_ID: [u8; PUBKEY_LENGTH] = [0u8; PUBKEY_LENGTH];
const BPF_LOADER_UPGRADEABLE_PROGRAM_ID: [u8; PUBKEY_LENGTH] = [
    0x02, 0xA8, 0xF6, 0x91, 0x4E, 0x88, 0xA1, 0xB0, 0xE2, 0x10, 0x15, 0x3E, 0xF7, 0x63, 0xAE, 0x2B,
    0x00, 0xC2, 0xB9, 0x3D, 0x16, 0xC1, 0x24, 0xD2, 0xC0, 0x53, 0x7A, 0x10, 0x04, 0x80, 0x00, 0x00,
];
const SYSVAR_RENT_ID: [u8; PUBKEY_LENGTH] = [
    0x06, 0xA7, 0xD5, 0x17, 0x19, 0x2C, 0x5C, 0x51, 0x21, 0x8C, 0xC9, 0x4C, 0x3D, 0x4A, 0xF1, 0x7F,
    0x58, 0xDA, 0xEE, 0x08, 0x9B, 0xA1, 0xFD, 0x44, 0xE3, 0xDB, 0xD9, 0x8A, 0x00, 0x00, 0x00, 0x00,
];
const SYSVAR_CLOCK_ID: [u8; PUBKEY_LENGTH] = [
    0x06, 0xA7, 0xD5, 0x17, 0x18, 0xC7, 0x74, 0xC9, 0x28, 0x56, 0x63, 0x98, 0x69, 0x1D, 0x5E, 0xB6,
    0x8B, 0x5E, 0xB8, 0xA3, 0x9B, 0x4B, 0x6D, 0x5C, 0x73, 0x55, 0x5B, 0x21, 0x00, 0x00, 0x00, 0x00,
];
const SQUADS_PROGRAM_ID: [u8; PUBKEY_LENGTH] = [
    0x06, 0x81, 0xC4, 0xCE, 0x47, 0xE2, 0x23, 0x68, 0xB8, 0xB1, 0x55, 0x5E, 0xC8, 0x87, 0xAF, 0x09,
    0x2E, 0xFC, 0x7E, 0xFB, 0xB6, 0x6C, 0xA3, 0xF5, 0x2F, 0xBF, 0x68, 0xD4, 0xAC, 0x9C, 0xB7, 0xA8,
];
const PROPOSAL_APPROVE_DISCRIMINATOR: [u8; 8] = [144, 37, 164, 136, 188, 216, 42, 248];
const PROPOSAL_REJECT_DISCRIMINATOR: [u8; 8] = [243, 62, 134, 156, 230, 106, 246, 135];
const VAULT_TRANSACTION_CREATE_DISCRIMINATOR: [u8; 8] = [48, 250, 78, 168, 208, 226, 218, 211];
const PROPOSAL_CREATE_DISCRIMINATOR: [u8; 8] = [220, 60, 73, 224, 30, 108, 79, 159];
const VAULT_TRANSACTION_EXECUTE_DISCRIMINATOR: [u8; 8] = [194, 8, 161, 87, 153, 164, 25, 171];
const UPGRADEABLE_LOADER_UPGRADE_IX: [u8; 4] = [3, 0, 0, 0];
const PDA_MARKER: &[u8] = b"ProgramDerivedAddress";
const SEED_PREFIX: &[u8] = b"multisig";
const SEED_TRANSACTION: &[u8] = b"transaction";
const SEED_PROPOSAL: &[u8] = b"proposal";
const SEED_VAULT: &[u8] = b"vault";

pub struct ProposalVoteArtifacts {
    pub proposal: [u8; PUBKEY_LENGTH],
    pub message_hash: [u8; MESSAGE_HASH_LENGTH],
    pub signature: [u8; SIGNATURE_LENGTH],
}

pub struct ProposalCreateUpgradeArtifacts {
    pub intent_hash: [u8; MESSAGE_HASH_LENGTH],
    pub create_message_hash: [u8; MESSAGE_HASH_LENGTH],
    pub proposal_message_hash: [u8; MESSAGE_HASH_LENGTH],
    pub create_signature: [u8; SIGNATURE_LENGTH],
    pub proposal_signature: [u8; SIGNATURE_LENGTH],
}

pub struct ProposalExecuteUpgradeArtifacts {
    pub intent_hash: [u8; MESSAGE_HASH_LENGTH],
    pub message_hash: [u8; MESSAGE_HASH_LENGTH],
    pub signature: [u8; SIGNATURE_LENGTH],
}

struct LegacyMessageBuilder {
    bytes: ArrayVec<u8, MAX_MESSAGE_LENGTH>,
}

impl LegacyMessageBuilder {
    fn new() -> Self {
        Self {
            bytes: ArrayVec::new(),
        }
    }

    fn push_u8(&mut self, value: u8) -> Result<(), AppSW> {
        self.bytes.try_push(value).map_err(|_| AppSW::CommError)
    }

    fn push_bytes(&mut self, value: &[u8]) -> Result<(), AppSW> {
        self.bytes
            .try_extend_from_slice(value)
            .map_err(|_| AppSW::CommError)
    }

    fn push_shortvec(&mut self, mut value: usize) -> Result<(), AppSW> {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            self.push_u8(byte)?;
            if value == 0 {
                return Ok(());
            }
        }
    }

    fn push_legacy_message(
        mut self,
        required_signatures: u8,
        readonly_signed_accounts: u8,
        readonly_unsigned_accounts: u8,
        accounts: &[&[u8; PUBKEY_LENGTH]],
        recent_blockhash: &[u8; BLOCKHASH_LENGTH],
        program_id_index: u8,
        account_indexes: &[u8],
        instruction_data: &[u8],
    ) -> Result<ArrayVec<u8, MAX_MESSAGE_LENGTH>, AppSW> {
        self.push_u8(required_signatures)?;
        self.push_u8(readonly_signed_accounts)?;
        self.push_u8(readonly_unsigned_accounts)?;
        self.push_shortvec(accounts.len())?;
        for account in accounts {
            self.push_bytes(account.as_slice())?;
        }
        self.push_bytes(recent_blockhash)?;
        self.push_u8(1)?;
        self.push_u8(program_id_index)?;
        self.push_shortvec(account_indexes.len())?;
        self.push_bytes(account_indexes)?;
        self.push_shortvec(instruction_data.len())?;
        self.push_bytes(instruction_data)?;
        Ok(self.bytes)
    }
}

pub fn derive_proposal_pda(
    multisig: &[u8; PUBKEY_LENGTH],
    transaction_index: u64,
) -> Result<[u8; PUBKEY_LENGTH], AppSW> {
    let tx_index = transaction_index.to_le_bytes();
    derive_pda(
        &[
            SEED_PREFIX,
            multisig,
            SEED_TRANSACTION,
            tx_index.as_slice(),
            SEED_PROPOSAL,
        ],
        &SQUADS_PROGRAM_ID,
    )
}

pub fn derive_transaction_pda(
    multisig: &[u8; PUBKEY_LENGTH],
    transaction_index: u64,
) -> Result<[u8; PUBKEY_LENGTH], AppSW> {
    let tx_index = transaction_index.to_le_bytes();
    derive_pda(
        &[SEED_PREFIX, multisig, SEED_TRANSACTION, tx_index.as_slice()],
        &SQUADS_PROGRAM_ID,
    )
}

pub fn derive_vault_pda(
    multisig: &[u8; PUBKEY_LENGTH],
    vault_index: u8,
) -> Result<[u8; PUBKEY_LENGTH], AppSW> {
    let vault_index_seed = [vault_index];
    derive_pda(
        &[
            SEED_PREFIX,
            multisig,
            SEED_VAULT,
            vault_index_seed.as_slice(),
        ],
        &SQUADS_PROGRAM_ID,
    )
}

pub fn derive_program_data_pda(
    program: &[u8; PUBKEY_LENGTH],
) -> Result<[u8; PUBKEY_LENGTH], AppSW> {
    derive_pda(&[program], &BPF_LOADER_UPGRADEABLE_PROGRAM_ID)
}

pub fn build_proposal_vote_message(
    member: &[u8; PUBKEY_LENGTH],
    multisig: &[u8; PUBKEY_LENGTH],
    proposal: &[u8; PUBKEY_LENGTH],
    vote: u8,
    recent_blockhash: &[u8; BLOCKHASH_LENGTH],
) -> Result<ArrayVec<u8, MAX_MESSAGE_LENGTH>, AppSW> {
    let mut instruction_data = [0u8; 9];
    match vote {
        PROPOSAL_VOTE_APPROVE => {
            instruction_data[..8].copy_from_slice(&PROPOSAL_APPROVE_DISCRIMINATOR);
        }
        PROPOSAL_VOTE_REJECT => {
            instruction_data[..8].copy_from_slice(&PROPOSAL_REJECT_DISCRIMINATOR);
        }
        _ => return Err(AppSW::InvalidData),
    }

    let accounts = [member, proposal, &SQUADS_PROGRAM_ID, multisig];
    let account_indexes = [3u8, 0, 1];

    LegacyMessageBuilder::new().push_legacy_message(
        1,
        0,
        2,
        &accounts,
        recent_blockhash,
        2,
        &account_indexes,
        &instruction_data,
    )
}

pub fn sign_message_with_path(
    derivation_path: &[u32; MAX_DERIVATION_PATH_LENGTH],
    path_length: u8,
    message: &[u8],
) -> Result<[u8; SIGNATURE_LENGTH], AppSW> {
    let sk = Ed25519::derive_from_path_slip10(&derivation_path[..path_length as usize]);
    let (signature, signature_length) = sk.sign(message).map_err(|_| AppSW::KeyDeriveFail)?;
    if signature_length as usize != SIGNATURE_LENGTH {
        return Err(AppSW::CommError);
    }
    Ok(signature)
}

pub fn sha256_bytes(data: &[u8]) -> Result<[u8; MESSAGE_HASH_LENGTH], AppSW> {
    let mut sha = Sha2_256::new();
    let mut out = [0u8; MESSAGE_HASH_LENGTH];
    sha.hash(data, &mut out).map_err(|_| AppSW::CommError)?;
    Ok(out)
}

pub fn build_proposal_vote_artifacts(
    member: &[u8; PUBKEY_LENGTH],
    multisig: &[u8; PUBKEY_LENGTH],
    derivation_path: &[u32; MAX_DERIVATION_PATH_LENGTH],
    path_length: u8,
    transaction_index: u64,
    vote: u8,
    recent_blockhash: &[u8; BLOCKHASH_LENGTH],
) -> Result<ProposalVoteArtifacts, AppSW> {
    let proposal = derive_proposal_pda(multisig, transaction_index)?;
    let message = build_proposal_vote_message(member, multisig, &proposal, vote, recent_blockhash)?;
    let message_hash = sha256_bytes(message.as_slice())?;
    let signature = sign_message_with_path(derivation_path, path_length, message.as_slice())?;

    Ok(ProposalVoteArtifacts {
        proposal,
        message_hash,
        signature,
    })
}

pub fn build_upgrade_intent_hash(
    multisig: &[u8; PUBKEY_LENGTH],
    vault_index: u8,
    program: &[u8; PUBKEY_LENGTH],
    buffer: &[u8; PUBKEY_LENGTH],
    spill: &[u8; PUBKEY_LENGTH],
) -> Result<[u8; MESSAGE_HASH_LENGTH], AppSW> {
    let vault = derive_vault_pda(multisig, vault_index)?;
    let wrapped_message = build_upgrade_wrapped_message(&vault, program, buffer, spill)?;
    sha256_bytes(wrapped_message.as_slice())
}

pub fn build_proposal_create_upgrade_artifacts(
    member: &[u8; PUBKEY_LENGTH],
    multisig: &[u8; PUBKEY_LENGTH],
    derivation_path: &[u32; MAX_DERIVATION_PATH_LENGTH],
    path_length: u8,
    transaction_index: u64,
    vault_index: u8,
    program: &[u8; PUBKEY_LENGTH],
    buffer: &[u8; PUBKEY_LENGTH],
    spill: &[u8; PUBKEY_LENGTH],
    transaction_blockhash: &[u8; BLOCKHASH_LENGTH],
    proposal_blockhash: &[u8; BLOCKHASH_LENGTH],
) -> Result<ProposalCreateUpgradeArtifacts, AppSW> {
    let transaction = derive_transaction_pda(multisig, transaction_index)?;
    let proposal = derive_proposal_pda(multisig, transaction_index)?;
    let intent_hash = build_upgrade_intent_hash(multisig, vault_index, program, buffer, spill)?;

    let create_message = build_upgrade_create_transaction_message(
        member,
        multisig,
        &transaction,
        vault_index,
        program,
        buffer,
        spill,
        transaction_blockhash,
    )?;
    let proposal_message = build_proposal_create_message(
        member,
        multisig,
        &proposal,
        transaction_index,
        proposal_blockhash,
    )?;

    let create_message_hash = sha256_bytes(create_message.as_slice())?;
    let proposal_message_hash = sha256_bytes(proposal_message.as_slice())?;
    let create_signature =
        sign_message_with_path(derivation_path, path_length, create_message.as_slice())?;
    let proposal_signature =
        sign_message_with_path(derivation_path, path_length, proposal_message.as_slice())?;

    Ok(ProposalCreateUpgradeArtifacts {
        intent_hash,
        create_message_hash,
        proposal_message_hash,
        create_signature,
        proposal_signature,
    })
}

pub struct ExecuteUpgradeInputs<'a> {
    pub member: &'a [u8; PUBKEY_LENGTH],
    pub multisig: &'a [u8; PUBKEY_LENGTH],
    pub derivation_path: &'a [u32; MAX_DERIVATION_PATH_LENGTH],
    pub path_length: u8,
    pub transaction_index: u64,
    pub vault_index: u8,
    pub program: &'a [u8; PUBKEY_LENGTH],
    pub buffer: &'a [u8; PUBKEY_LENGTH],
    pub spill: &'a [u8; PUBKEY_LENGTH],
    pub recent_blockhash: &'a [u8; BLOCKHASH_LENGTH],
}

pub fn build_proposal_execute_upgrade_artifacts(
    inputs: ExecuteUpgradeInputs<'_>,
) -> Result<ProposalExecuteUpgradeArtifacts, AppSW> {
    let transaction = derive_transaction_pda(inputs.multisig, inputs.transaction_index)?;
    let proposal = derive_proposal_pda(inputs.multisig, inputs.transaction_index)?;
    let intent_hash = build_upgrade_intent_hash(
        inputs.multisig,
        inputs.vault_index,
        inputs.program,
        inputs.buffer,
        inputs.spill,
    )?;
    let message = build_upgrade_execute_message(
        inputs.member,
        inputs.multisig,
        &proposal,
        &transaction,
        inputs.vault_index,
        inputs.program,
        inputs.buffer,
        inputs.spill,
        inputs.recent_blockhash,
    )?;
    let message_hash = sha256_bytes(message.as_slice())?;
    let signature = sign_message_with_path(
        inputs.derivation_path,
        inputs.path_length,
        message.as_slice(),
    )?;

    Ok(ProposalExecuteUpgradeArtifacts {
        intent_hash,
        message_hash,
        signature,
    })
}

fn build_upgrade_wrapped_message(
    vault: &[u8; PUBKEY_LENGTH],
    program: &[u8; PUBKEY_LENGTH],
    buffer: &[u8; PUBKEY_LENGTH],
    spill: &[u8; PUBKEY_LENGTH],
) -> Result<ArrayVec<u8, MAX_MESSAGE_LENGTH>, AppSW> {
    let program_data = derive_program_data_pda(program)?;
    let accounts = [
        vault,
        &program_data,
        program,
        buffer,
        spill,
        &BPF_LOADER_UPGRADEABLE_PROGRAM_ID,
        &SYSVAR_RENT_ID,
        &SYSVAR_CLOCK_ID,
    ];
    let account_indexes = [1u8, 2, 3, 4, 6, 7, 0];

    let mut builder = LegacyMessageBuilder::new();
    builder.push_u8(1)?;
    builder.push_u8(1)?;
    builder.push_u8(4)?;
    builder.push_u8(8)?;
    for account in accounts {
        builder.push_bytes(account)?;
    }
    builder.push_u8(1)?;
    builder.push_u8(5)?;
    builder.push_u8(account_indexes.len() as u8)?;
    builder.push_bytes(&account_indexes)?;
    builder.push_bytes(&(UPGRADEABLE_LOADER_UPGRADE_IX.len() as u16).to_le_bytes())?;
    builder.push_bytes(&UPGRADEABLE_LOADER_UPGRADE_IX)?;
    builder.push_u8(0)?;
    Ok(builder.bytes)
}

fn build_upgrade_create_transaction_message(
    member: &[u8; PUBKEY_LENGTH],
    multisig: &[u8; PUBKEY_LENGTH],
    transaction: &[u8; PUBKEY_LENGTH],
    vault_index: u8,
    program: &[u8; PUBKEY_LENGTH],
    buffer: &[u8; PUBKEY_LENGTH],
    spill: &[u8; PUBKEY_LENGTH],
    recent_blockhash: &[u8; BLOCKHASH_LENGTH],
) -> Result<ArrayVec<u8, MAX_MESSAGE_LENGTH>, AppSW> {
    let vault = derive_vault_pda(multisig, vault_index)?;
    let wrapped_message = build_upgrade_wrapped_message(&vault, program, buffer, spill)?;
    let accounts = [
        member,
        multisig,
        transaction,
        &SQUADS_PROGRAM_ID,
        &SYSTEM_PROGRAM_ID,
    ];
    let account_indexes = [1u8, 2, 0, 0, 4];

    let mut instruction_data = ArrayVec::<u8, MAX_MESSAGE_LENGTH>::new();
    instruction_data
        .try_extend_from_slice(&VAULT_TRANSACTION_CREATE_DISCRIMINATOR)
        .map_err(|_| AppSW::CommError)?;
    instruction_data
        .try_push(vault_index)
        .map_err(|_| AppSW::CommError)?;
    instruction_data.try_push(0).map_err(|_| AppSW::CommError)?;
    instruction_data
        .try_extend_from_slice(&(wrapped_message.len() as u32).to_le_bytes())
        .map_err(|_| AppSW::CommError)?;
    instruction_data
        .try_extend_from_slice(wrapped_message.as_slice())
        .map_err(|_| AppSW::CommError)?;
    instruction_data.try_push(0).map_err(|_| AppSW::CommError)?;

    LegacyMessageBuilder::new().push_legacy_message(
        1,
        0,
        2,
        &accounts,
        recent_blockhash,
        3,
        &account_indexes,
        instruction_data.as_slice(),
    )
}

fn build_proposal_create_message(
    member: &[u8; PUBKEY_LENGTH],
    multisig: &[u8; PUBKEY_LENGTH],
    proposal: &[u8; PUBKEY_LENGTH],
    transaction_index: u64,
    recent_blockhash: &[u8; BLOCKHASH_LENGTH],
) -> Result<ArrayVec<u8, MAX_MESSAGE_LENGTH>, AppSW> {
    let accounts = [
        member,
        proposal,
        &SQUADS_PROGRAM_ID,
        multisig,
        &SYSTEM_PROGRAM_ID,
    ];
    let account_indexes = [3u8, 1, 0, 0, 4];

    let mut instruction_data = ArrayVec::<u8, 17>::new();
    instruction_data
        .try_extend_from_slice(&PROPOSAL_CREATE_DISCRIMINATOR)
        .map_err(|_| AppSW::CommError)?;
    instruction_data
        .try_extend_from_slice(&transaction_index.to_le_bytes())
        .map_err(|_| AppSW::CommError)?;
    instruction_data.try_push(0).map_err(|_| AppSW::CommError)?;

    LegacyMessageBuilder::new().push_legacy_message(
        1,
        0,
        3,
        &accounts,
        recent_blockhash,
        2,
        &account_indexes,
        instruction_data.as_slice(),
    )
}

fn build_upgrade_execute_message(
    member: &[u8; PUBKEY_LENGTH],
    multisig: &[u8; PUBKEY_LENGTH],
    proposal: &[u8; PUBKEY_LENGTH],
    transaction: &[u8; PUBKEY_LENGTH],
    vault_index: u8,
    program: &[u8; PUBKEY_LENGTH],
    buffer: &[u8; PUBKEY_LENGTH],
    spill: &[u8; PUBKEY_LENGTH],
    recent_blockhash: &[u8; BLOCKHASH_LENGTH],
) -> Result<ArrayVec<u8, MAX_MESSAGE_LENGTH>, AppSW> {
    let vault = derive_vault_pda(multisig, vault_index)?;
    let program_data = derive_program_data_pda(program)?;
    let accounts = [
        member,
        proposal,
        &vault,
        &program_data,
        program,
        buffer,
        spill,
        &SQUADS_PROGRAM_ID,
        multisig,
        transaction,
        &BPF_LOADER_UPGRADEABLE_PROGRAM_ID,
        &SYSVAR_RENT_ID,
        &SYSVAR_CLOCK_ID,
    ];
    let account_indexes = [8u8, 1, 9, 0, 2, 3, 4, 5, 6, 10, 11, 12];

    LegacyMessageBuilder::new().push_legacy_message(
        1,
        0,
        6,
        &accounts,
        recent_blockhash,
        7,
        &account_indexes,
        &VAULT_TRANSACTION_EXECUTE_DISCRIMINATOR,
    )
}

fn derive_pda(
    seeds: &[&[u8]],
    program_id: &[u8; PUBKEY_LENGTH],
) -> Result<[u8; PUBKEY_LENGTH], AppSW> {
    for bump in (0..=255u8).rev() {
        let bump_seed = [bump];
        let mut sha = Sha2_256::new();
        for seed in seeds {
            sha.update(seed).map_err(|_| AppSW::CommError)?;
        }
        sha.update(&bump_seed).map_err(|_| AppSW::CommError)?;
        sha.update(program_id).map_err(|_| AppSW::CommError)?;
        sha.update(PDA_MARKER).map_err(|_| AppSW::CommError)?;

        let mut candidate = [0u8; PUBKEY_LENGTH];
        sha.finalize(&mut candidate).map_err(|_| AppSW::CommError)?;
        if !is_on_curve(&candidate) {
            return Ok(candidate);
        }
    }

    Err(AppSW::CommError)
}

fn is_on_curve(compressed_point: &[u8; PUBKEY_LENGTH]) -> bool {
    let mut point = cx_ecpoint_t::default();
    let mut local = *compressed_point;
    let mut on_curve = false;

    let alloc = unsafe { cx_ecpoint_alloc(&mut point as *mut cx_ecpoint_t, CX_CURVE_Ed25519) };
    if alloc != CX_OK {
        return false;
    }

    let sign = unsafe { cx_decode_coord(local.as_mut_ptr(), local.len() as i32) };
    let decompress = unsafe {
        cx_ecpoint_decompress(
            &mut point as *mut cx_ecpoint_t,
            local.as_mut_ptr(),
            local.len(),
            sign as u32,
        )
    };
    if decompress != CX_OK {
        return false;
    }

    let check = unsafe {
        cx_ecpoint_is_on_curve(&point as *const cx_ecpoint_t, &mut on_curve as *mut bool)
    };
    check == CX_OK && on_curve
}
