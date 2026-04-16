use crate::AppSW;
use ledger_device_sdk::ecc::Ed25519;

use crate::storage::{SavedMultisigEntry, MAX_DERIVATION_PATH_LENGTH, PUBKEY_LENGTH};
use crate::squads_tx::{
    BLOCKHASH_LENGTH, PROPOSAL_VOTE_APPROVE, PROPOSAL_VOTE_REJECT,
};

pub const APP_CLA: u8 = 0xe0;
pub const P1_CONFIRM: u8 = 0x00;
pub const P1_NON_CONFIRM: u8 = 0x01;

pub const INS_GET_VERSION: u8 = 0x00;
pub const INS_SAVE_MULTISIG: u8 = 0x10;
pub const INS_LIST_MULTISIG_SLOT: u8 = 0x11;
pub const INS_PROPOSAL_VOTE: u8 = 0x12;
pub const INS_RESET_MULTISIGS: u8 = 0x13;
pub const INS_PROPOSAL_CREATE_UPGRADE: u8 = 0x14;
pub const INS_PROPOSAL_EXECUTE_UPGRADE: u8 = 0x15;

pub struct ProposalVoteRequest {
    pub multisig: [u8; PUBKEY_LENGTH],
    pub transaction_index: u64,
    pub vote: u8,
    pub recent_blockhash: [u8; BLOCKHASH_LENGTH],
    pub fee_payer: Option<[u8; PUBKEY_LENGTH]>,
}

pub struct ProposalCreateUpgradeRequest {
    pub multisig: [u8; PUBKEY_LENGTH],
    pub transaction_index: u64,
    pub vault_index: u8,
    pub program: [u8; PUBKEY_LENGTH],
    pub buffer: [u8; PUBKEY_LENGTH],
    pub spill: [u8; PUBKEY_LENGTH],
    pub transaction_blockhash: [u8; BLOCKHASH_LENGTH],
    pub proposal_blockhash: [u8; BLOCKHASH_LENGTH],
}

pub struct ProposalExecuteUpgradeRequest {
    pub multisig: [u8; PUBKEY_LENGTH],
    pub transaction_index: u64,
    pub vault_index: u8,
    pub program: [u8; PUBKEY_LENGTH],
    pub buffer: [u8; PUBKEY_LENGTH],
    pub spill: [u8; PUBKEY_LENGTH],
    pub blockhash: [u8; BLOCKHASH_LENGTH],
}

pub fn parse_derivation_path(data: &[u8]) -> Result<([u32; MAX_DERIVATION_PATH_LENGTH], u8, usize), AppSW> {
    if data.is_empty() {
        return Err(AppSW::WrongApduLength);
    }

    let path_len = data[0] as usize;
    if path_len == 0 || path_len > MAX_DERIVATION_PATH_LENGTH {
        return Err(AppSW::WrongApduLength);
    }

    let encoded_len = 1 + path_len * 4;
    if data.len() < encoded_len {
        return Err(AppSW::WrongApduLength);
    }

    let mut path = [0u32; MAX_DERIVATION_PATH_LENGTH];
    for (index, chunk) in data[1..encoded_len].chunks(4).enumerate() {
        path[index] = u32::from_be_bytes(chunk.try_into().map_err(|_| AppSW::WrongApduLength)?);
    }

    if path[0] != 0x8000_002c || path[1] != 0x8000_01f5 {
        return Err(AppSW::WrongApduLength);
    }

    Ok((path, path_len as u8, encoded_len))
}

pub fn derive_member_pubkey(path: &[u32]) -> Result<[u8; PUBKEY_LENGTH], AppSW> {
    let sk = Ed25519::derive_from_path_slip10(path);
    let pk = sk.public_key().map_err(|_| AppSW::KeyDeriveFail)?;
    let mut out = [0u8; PUBKEY_LENGTH];
    out.copy_from_slice(&pk.pubkey[1..33]);
    Ok(out)
}

pub fn make_saved_entry(data: &[u8]) -> Result<SavedMultisigEntry, AppSW> {
    let (derivation_path, path_length, consumed) = parse_derivation_path(data)?;
    if data.len() != consumed + PUBKEY_LENGTH {
        return Err(AppSW::WrongApduLength);
    }

    let mut multisig = [0u8; PUBKEY_LENGTH];
    multisig.copy_from_slice(&data[consumed..consumed + PUBKEY_LENGTH]);

    let member = derive_member_pubkey(&derivation_path[..path_length as usize])?;

    Ok(SavedMultisigEntry {
        occupied: true,
        multisig,
        member,
        path_length,
        derivation_path,
    })
}

pub fn parse_proposal_vote_request(data: &[u8]) -> Result<ProposalVoteRequest, AppSW> {
    if data.len() != 74 && data.len() != 106 {
        return Err(AppSW::InvalidData);
    }

    let mut multisig = [0u8; PUBKEY_LENGTH];
    multisig.copy_from_slice(&data[..PUBKEY_LENGTH]);
    let transaction_index = u64::from_le_bytes(
        data[PUBKEY_LENGTH..PUBKEY_LENGTH + 8]
            .try_into()
            .map_err(|_| AppSW::InvalidData)?,
    );
    let vote = data[40];
    if vote != PROPOSAL_VOTE_APPROVE && vote != PROPOSAL_VOTE_REJECT {
        return Err(AppSW::InvalidData);
    }

    let mut recent_blockhash = [0u8; BLOCKHASH_LENGTH];
    recent_blockhash.copy_from_slice(&data[41..73]);

    let fee_payer_present = data[73];
    let fee_payer = match fee_payer_present {
        0 => None,
        1 => {
            if data.len() != 106 {
                return Err(AppSW::InvalidData);
            }
            let mut fee_payer = [0u8; PUBKEY_LENGTH];
            fee_payer.copy_from_slice(&data[74..106]);
            Some(fee_payer)
        }
        _ => return Err(AppSW::InvalidData),
    };

    Ok(ProposalVoteRequest {
        multisig,
        transaction_index,
        vote,
        recent_blockhash,
        fee_payer,
    })
}

pub fn parse_proposal_create_upgrade_request(
    data: &[u8],
) -> Result<ProposalCreateUpgradeRequest, AppSW> {
    if data.len() != 201 {
        return Err(AppSW::InvalidData);
    }

    let mut multisig = [0u8; PUBKEY_LENGTH];
    multisig.copy_from_slice(&data[..32]);
    let transaction_index = u64::from_le_bytes(data[32..40].try_into().map_err(|_| AppSW::InvalidData)?);
    let vault_index = data[40];

    let mut program = [0u8; PUBKEY_LENGTH];
    program.copy_from_slice(&data[41..73]);
    let mut buffer = [0u8; PUBKEY_LENGTH];
    buffer.copy_from_slice(&data[73..105]);
    let mut spill = [0u8; PUBKEY_LENGTH];
    spill.copy_from_slice(&data[105..137]);
    let mut transaction_blockhash = [0u8; BLOCKHASH_LENGTH];
    transaction_blockhash.copy_from_slice(&data[137..169]);
    let mut proposal_blockhash = [0u8; BLOCKHASH_LENGTH];
    proposal_blockhash.copy_from_slice(&data[169..201]);

    Ok(ProposalCreateUpgradeRequest {
        multisig,
        transaction_index,
        vault_index,
        program,
        buffer,
        spill,
        transaction_blockhash,
        proposal_blockhash,
    })
}

pub fn parse_proposal_execute_upgrade_request(
    data: &[u8],
) -> Result<ProposalExecuteUpgradeRequest, AppSW> {
    if data.len() != 169 {
        return Err(AppSW::InvalidData);
    }

    let mut multisig = [0u8; PUBKEY_LENGTH];
    multisig.copy_from_slice(&data[..32]);
    let transaction_index = u64::from_le_bytes(data[32..40].try_into().map_err(|_| AppSW::InvalidData)?);
    let vault_index = data[40];

    let mut program = [0u8; PUBKEY_LENGTH];
    program.copy_from_slice(&data[41..73]);
    let mut buffer = [0u8; PUBKEY_LENGTH];
    buffer.copy_from_slice(&data[73..105]);
    let mut spill = [0u8; PUBKEY_LENGTH];
    spill.copy_from_slice(&data[105..137]);
    let mut blockhash = [0u8; BLOCKHASH_LENGTH];
    blockhash.copy_from_slice(&data[137..169]);

    Ok(ProposalExecuteUpgradeRequest {
        multisig,
        transaction_index,
        vault_index,
        program,
        buffer,
        spill,
        blockhash,
    })
}
