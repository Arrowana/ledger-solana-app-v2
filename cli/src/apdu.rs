use anyhow::{bail, Result};

use crate::constants::{AppInstruction, ProposalVote};
use crate::derivation::serialize_derivation_path;

#[derive(Debug, Clone)]
pub struct SavedEntry {
    pub slot: u8,
    pub multisig: [u8; 32],
    pub member: [u8; 32],
    pub path: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct ProposalVoteResponse {
    pub signature: [u8; 64],
    pub member: [u8; 32],
    pub proposal: [u8; 32],
    pub message_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct ProposalCreateUpgradeResponse {
    pub create_signature: [u8; 64],
    pub proposal_signature: [u8; 64],
    pub intent_hash: [u8; 32],
    pub create_message_hash: [u8; 32],
    pub proposal_message_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct ProposalExecuteUpgradeResponse {
    pub signature: [u8; 64],
    pub intent_hash: [u8; 32],
    pub message_hash: [u8; 32],
}

pub fn encode_apdu(
    instruction: AppInstruction,
    p1: u8,
    p2: u8,
    data: &[u8],
) -> Result<Vec<u8>> {
    if data.len() > u8::MAX as usize {
        bail!("APDU payload too large: {}", data.len());
    }

    let mut out = Vec::with_capacity(5 + data.len());
    out.extend_from_slice(&[
        crate::constants::APP_CLA,
        instruction as u8,
        p1,
        p2,
        data.len() as u8,
    ]);
    out.extend_from_slice(data);
    Ok(out)
}

pub fn decode_apdu_response(response: &[u8]) -> Result<(&[u8], u16)> {
    if response.len() < 2 {
        bail!("APDU response too short");
    }
    let split = response.len() - 2;
    Ok((&response[..split], u16::from_be_bytes([response[split], response[split + 1]])))
}

pub fn build_save_multisig_apdu(
    multisig: &[u8; 32],
    derivation_path: &[u32],
    non_confirm: bool,
) -> Result<Vec<u8>> {
    let mut payload = serialize_derivation_path(derivation_path)?;
    payload.extend_from_slice(multisig);
    encode_apdu(
        AppInstruction::SaveMultisig,
        if non_confirm { 1 } else { 0 },
        0,
        &payload,
    )
}

pub fn decode_save_multisig_response(response: &[u8]) -> Result<(u8, [u8; 32])> {
    if response.len() != 33 {
        bail!("unexpected save response length: {}", response.len());
    }

    let mut member = [0u8; 32];
    member.copy_from_slice(&response[1..33]);
    Ok((response[0], member))
}

pub fn build_list_multisig_slot_apdu(slot: u8) -> Result<Vec<u8>> {
    encode_apdu(AppInstruction::ListMultisigSlot, 0, slot, &[])
}

pub fn decode_list_multisig_slot_response(slot: u8, response: &[u8]) -> Result<Option<SavedEntry>> {
    if response.len() == 1 && response[0] == 0 {
        return Ok(None);
    }
    if response.len() < 67 {
        bail!("unexpected list response length: {}", response.len());
    }
    if response[0] != 1 {
        bail!("unexpected slot occupancy marker: {}", response[0]);
    }

    let path_length = response[65] as usize;
    let expected_length = 66 + path_length * 4;
    if response.len() != expected_length {
        bail!("unexpected list response path payload length: {}", response.len());
    }

    let mut multisig = [0u8; 32];
    multisig.copy_from_slice(&response[1..33]);
    let mut member = [0u8; 32];
    member.copy_from_slice(&response[33..65]);

    let mut path = Vec::with_capacity(path_length);
    let mut offset = 66;
    for _ in 0..path_length {
        path.push(u32::from_be_bytes([
            response[offset],
            response[offset + 1],
            response[offset + 2],
            response[offset + 3],
        ]));
        offset += 4;
    }

    Ok(Some(SavedEntry {
        slot,
        multisig,
        member,
        path,
    }))
}

pub fn build_proposal_vote_apdu(
    multisig: &[u8; 32],
    transaction_index: u64,
    vote: ProposalVote,
    blockhash: &[u8; 32],
    fee_payer: Option<&[u8; 32]>,
    non_confirm: bool,
) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(106);
    payload.extend_from_slice(multisig);
    payload.extend_from_slice(&transaction_index.to_le_bytes());
    payload.push(vote as u8);
    payload.extend_from_slice(blockhash);
    payload.push(u8::from(fee_payer.is_some()));
    if let Some(fee_payer) = fee_payer {
        payload.extend_from_slice(fee_payer);
    }

    encode_apdu(
        AppInstruction::ProposalVote,
        if non_confirm { 1 } else { 0 },
        0,
        &payload,
    )
}

pub fn decode_proposal_vote_response(response: &[u8]) -> Result<ProposalVoteResponse> {
    if response.len() != 160 {
        bail!("unexpected proposal vote response length: {}", response.len());
    }

    let mut signature = [0u8; 64];
    signature.copy_from_slice(&response[0..64]);
    let mut member = [0u8; 32];
    member.copy_from_slice(&response[64..96]);
    let mut proposal = [0u8; 32];
    proposal.copy_from_slice(&response[96..128]);
    let mut message_hash = [0u8; 32];
    message_hash.copy_from_slice(&response[128..160]);

    Ok(ProposalVoteResponse {
        signature,
        member,
        proposal,
        message_hash,
    })
}

pub struct ProposalCreateUpgradeRequest<'a> {
    pub multisig: &'a [u8; 32],
    pub transaction_index: u64,
    pub vault_index: u8,
    pub program: &'a [u8; 32],
    pub buffer: &'a [u8; 32],
    pub spill: &'a [u8; 32],
    pub transaction_blockhash: &'a [u8; 32],
    pub proposal_blockhash: &'a [u8; 32],
    pub non_confirm: bool,
}

pub fn build_proposal_create_upgrade_apdu(args: ProposalCreateUpgradeRequest<'_>) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(170);
    payload.extend_from_slice(args.multisig);
    payload.extend_from_slice(&args.transaction_index.to_le_bytes());
    payload.push(args.vault_index);
    payload.extend_from_slice(args.program);
    payload.extend_from_slice(args.buffer);
    payload.extend_from_slice(args.spill);
    payload.extend_from_slice(args.transaction_blockhash);
    payload.extend_from_slice(args.proposal_blockhash);

    encode_apdu(
        AppInstruction::ProposalCreateUpgrade,
        if args.non_confirm { 1 } else { 0 },
        0,
        &payload,
    )
}

pub fn decode_proposal_create_upgrade_response(
    response: &[u8],
) -> Result<ProposalCreateUpgradeResponse> {
    if response.len() != 224 {
        bail!(
            "unexpected proposal create upgrade response length: {}",
            response.len()
        );
    }

    let mut create_signature = [0u8; 64];
    create_signature.copy_from_slice(&response[0..64]);
    let mut proposal_signature = [0u8; 64];
    proposal_signature.copy_from_slice(&response[64..128]);
    let mut intent_hash = [0u8; 32];
    intent_hash.copy_from_slice(&response[128..160]);
    let mut create_message_hash = [0u8; 32];
    create_message_hash.copy_from_slice(&response[160..192]);
    let mut proposal_message_hash = [0u8; 32];
    proposal_message_hash.copy_from_slice(&response[192..224]);

    Ok(ProposalCreateUpgradeResponse {
        create_signature,
        proposal_signature,
        intent_hash,
        create_message_hash,
        proposal_message_hash,
    })
}

pub struct ProposalExecuteUpgradeRequest<'a> {
    pub multisig: &'a [u8; 32],
    pub transaction_index: u64,
    pub vault_index: u8,
    pub program: &'a [u8; 32],
    pub buffer: &'a [u8; 32],
    pub spill: &'a [u8; 32],
    pub blockhash: &'a [u8; 32],
    pub non_confirm: bool,
}

pub fn build_proposal_execute_upgrade_apdu(
    args: ProposalExecuteUpgradeRequest<'_>,
) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(138);
    payload.extend_from_slice(args.multisig);
    payload.extend_from_slice(&args.transaction_index.to_le_bytes());
    payload.push(args.vault_index);
    payload.extend_from_slice(args.program);
    payload.extend_from_slice(args.buffer);
    payload.extend_from_slice(args.spill);
    payload.extend_from_slice(args.blockhash);

    encode_apdu(
        AppInstruction::ProposalExecuteUpgrade,
        if args.non_confirm { 1 } else { 0 },
        0,
        &payload,
    )
}

pub fn decode_proposal_execute_upgrade_response(
    response: &[u8],
) -> Result<ProposalExecuteUpgradeResponse> {
    if response.len() != 128 {
        bail!(
            "unexpected proposal execute upgrade response length: {}",
            response.len()
        );
    }

    let mut signature = [0u8; 64];
    signature.copy_from_slice(&response[0..64]);
    let mut intent_hash = [0u8; 32];
    intent_hash.copy_from_slice(&response[64..96]);
    let mut message_hash = [0u8; 32];
    message_hash.copy_from_slice(&response[96..128]);

    Ok(ProposalExecuteUpgradeResponse {
        signature,
        intent_hash,
        message_hash,
    })
}

pub fn build_reset_multisigs_apdu(non_confirm: bool) -> Result<Vec<u8>> {
    encode_apdu(
        AppInstruction::ResetMultisigs,
        if non_confirm { 1 } else { 0 },
        0,
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::{build_list_multisig_slot_apdu, build_proposal_vote_apdu, decode_apdu_response};
    use crate::constants::ProposalVote;

    #[test]
    fn encodes_list_apdu() {
        let apdu = build_list_multisig_slot_apdu(3).unwrap();
        assert_eq!(hex::encode(apdu), "e011000300");
    }

    #[test]
    fn proposal_vote_roundtrip() {
        let multisig = [1u8; 32];
        let blockhash = [2u8; 32];
        let apdu =
            build_proposal_vote_apdu(&multisig, 42, ProposalVote::Approve, &blockhash, None, false)
                .unwrap();
        let (data, status) = decode_apdu_response(&[0xaa, 0xbb, 0x90, 0x00]).unwrap();
        assert_eq!(status, 0x9000);
        assert_eq!(data, &[0xaa, 0xbb]);
        assert_eq!(apdu[0], 0xe0);
        assert_eq!(apdu[1], 0x12);
    }
}

