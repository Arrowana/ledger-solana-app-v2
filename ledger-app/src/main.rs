/*****************************************************************************
 *   Ledger App Boilerplate Rust.
 *   (c) 2023 Ledger SAS.
 *
 *  Licensed under the Apache License, Version 2.0 (the "License");
 *  you may not use this file except in compliance with the License.
 *  You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 *  Unless required by applicable law or agreed to in writing, software
 *  distributed under the License is distributed on an "AS IS" BASIS,
 *  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 *  See the License for the specific language governing permissions and
 *  limitations under the License.
 *****************************************************************************/

#![no_std]
#![no_main]

mod app_ui {
    pub mod menu;
    pub mod squads;
}
mod settings;
mod squads;
mod squads_tx;
mod squads_types;
mod storage;

use app_ui::menu::ui_menu_main;
use app_ui::squads::{
    show_create_upgrade_review, show_execute_upgrade_review, show_reset_review, show_save_review,
    show_status, show_vote_review,
};
use ledger_device_sdk::io::{self, init_comm, ApduHeader, Command, Reply, StatusWords};
use squads::{
    make_saved_entry, APP_CLA, INS_GET_VERSION, INS_LIST_MULTISIG_SLOT, INS_PROPOSAL_CREATE_UPGRADE,
    INS_PROPOSAL_EXECUTE_UPGRADE, INS_PROPOSAL_VOTE, INS_RESET_MULTISIGS, INS_SAVE_MULTISIG,
    P1_CONFIRM, P1_NON_CONFIRM, parse_proposal_create_upgrade_request,
    parse_proposal_execute_upgrade_request, parse_proposal_vote_request,
};
use squads_tx::{
    build_proposal_create_upgrade_artifacts, build_proposal_execute_upgrade_artifacts,
    build_proposal_vote_artifacts, ExecuteUpgradeInputs, MESSAGE_HASH_LENGTH, SIGNATURE_LENGTH,
};
use storage::{ReviewedUpgradeIntent, Storage, MAX_SAVED_MULTISIGS};

ledger_device_sdk::set_panic!(ledger_device_sdk::exiting_panic);
ledger_device_sdk::define_comm!(COMM);

#[repr(u16)]
#[derive(Clone, Copy, PartialEq)]
pub enum AppSW {
    Deny = 0x6985,
    ConditionsNotSatisfied = 0x6986,
    InvalidData = 0x6A80,
    NotFound = 0x6A88,
    WrongP1P2 = 0x6A86,
    InsNotSupported = 0x6D00,
    ClaNotSupported = 0x6E00,
    CommError = 0x6F00,
    KeyDeriveFail = 0xB009,
    WrongApduLength = StatusWords::BadLen as u16,
    Ok = 0x9000,
}

impl From<AppSW> for Reply {
    fn from(sw: AppSW) -> Reply {
        Reply(sw as u16)
    }
}

impl From<io::CommError> for AppSW {
    fn from(_: io::CommError) -> Self {
        AppSW::CommError
    }
}

#[derive(Debug)]
pub enum Instruction {
    GetVersion,
    SaveMultisig { non_confirm: bool },
    ListMultisigSlot { slot: u8 },
    ProposalVote { non_confirm: bool },
    ResetMultisigs { non_confirm: bool },
    ProposalCreateUpgrade { non_confirm: bool },
    ProposalExecuteUpgrade { non_confirm: bool },
}

impl TryFrom<ApduHeader> for Instruction {
    type Error = AppSW;

    fn try_from(value: ApduHeader) -> Result<Self, Self::Error> {
        match (value.ins, value.p1, value.p2) {
            (INS_GET_VERSION, 0, 0) => Ok(Self::GetVersion),
            (INS_SAVE_MULTISIG, P1_CONFIRM | P1_NON_CONFIRM, 0) => Ok(Self::SaveMultisig {
                non_confirm: value.p1 == P1_NON_CONFIRM,
            }),
            (INS_LIST_MULTISIG_SLOT, 0, slot) if slot < MAX_SAVED_MULTISIGS as u8 => {
                Ok(Self::ListMultisigSlot { slot })
            }
            (INS_PROPOSAL_VOTE, P1_CONFIRM | P1_NON_CONFIRM, 0) => Ok(Self::ProposalVote {
                non_confirm: value.p1 == P1_NON_CONFIRM,
            }),
            (INS_RESET_MULTISIGS, P1_CONFIRM | P1_NON_CONFIRM, 0) => Ok(Self::ResetMultisigs {
                non_confirm: value.p1 == P1_NON_CONFIRM,
            }),
            (INS_PROPOSAL_CREATE_UPGRADE, P1_CONFIRM | P1_NON_CONFIRM, 0) => Ok(Self::ProposalCreateUpgrade {
                non_confirm: value.p1 == P1_NON_CONFIRM,
            }),
            (INS_PROPOSAL_EXECUTE_UPGRADE, P1_CONFIRM | P1_NON_CONFIRM, 0) => Ok(Self::ProposalExecuteUpgrade {
                non_confirm: value.p1 == P1_NON_CONFIRM,
            }),
            (INS_LIST_MULTISIG_SLOT, _, _) => Err(AppSW::WrongP1P2),
            (INS_SAVE_MULTISIG..=INS_PROPOSAL_EXECUTE_UPGRADE, _, _) => Err(AppSW::WrongP1P2),
            (APP_CLA, _, _) => Err(AppSW::InsNotSupported),
            (_, _, _) => Err(AppSW::InsNotSupported),
        }
    }
}

fn handle_save_multisig<'a>(
    command: Command<'a>,
    non_confirm: bool,
    storage: &Storage,
) -> Result<io::CommandResponse<'a>, AppSW> {
    let entry = make_saved_entry(command.get_data())?;
    let comm = command.into_comm();

    if !non_confirm && !show_save_review(comm, &entry) {
        return Err(AppSW::Deny);
    }

    let slot = storage.upsert(&entry).ok_or(AppSW::ConditionsNotSatisfied)?;
    if !non_confirm {
        show_status(comm, true);
    }

    let mut response = comm.begin_response();
    response.append(&[slot])?;
    response.append(&entry.member)?;
    Ok(response)
}

fn handle_list_multisig_slot<'a>(
    command: Command<'a>,
    slot: u8,
    storage: &Storage,
) -> Result<io::CommandResponse<'a>, AppSW> {
    let entry = storage.read_slot(slot).ok_or(AppSW::NotFound)?;
    let mut response = command.into_response();
    response.append(&[1])?;
    response.append(&entry.multisig)?;
    response.append(&entry.member)?;
    response.append(&[entry.path_length])?;
    for value in entry
        .derivation_path
        .iter()
        .take(entry.path_length as usize)
    {
        response.append(&value.to_be_bytes())?;
    }
    Ok(response)
}

fn handle_reset_multisigs<'a>(
    command: Command<'a>,
    non_confirm: bool,
    storage: &Storage,
) -> Result<io::CommandResponse<'a>, AppSW> {
    let comm = command.into_comm();
    if !non_confirm && !show_reset_review(comm) {
        return Err(AppSW::Deny);
    }

    storage.reset();
    if !non_confirm {
        show_status(comm, true);
    }
    Ok(comm.begin_response())
}

fn handle_proposal_vote<'a>(
    command: Command<'a>,
    non_confirm: bool,
    storage: &Storage,
) -> Result<io::CommandResponse<'a>, AppSW> {
    let request = parse_proposal_vote_request(command.get_data())?;
    let slot = storage
        .find_multisig(&request.multisig)
        .ok_or(AppSW::NotFound)?;
    let entry = storage.read_slot(slot).ok_or(AppSW::NotFound)?;

    if let Some(fee_payer) = request.fee_payer {
        if fee_payer != entry.member {
            return Err(AppSW::ConditionsNotSatisfied);
        }
    }

    let artifacts = build_proposal_vote_artifacts(
        &entry.member,
        &entry.multisig,
        &entry.derivation_path,
        entry.path_length,
        request.transaction_index,
        request.vote,
        &request.recent_blockhash,
    )?;

    let comm = command.into_comm();
    if !non_confirm
        && !show_vote_review(
            comm,
            &entry.multisig,
            &entry.member,
            request.transaction_index,
            request.vote,
            &artifacts.message_hash,
        )
    {
        return Err(AppSW::Deny);
    }

    if !non_confirm {
        show_status(comm, true);
    }

    let mut response = comm.begin_response();
    response.append(&artifacts.signature[..SIGNATURE_LENGTH])?;
    response.append(&entry.member)?;
    response.append(&artifacts.proposal)?;
    response.append(&artifacts.message_hash[..MESSAGE_HASH_LENGTH])?;
    Ok(response)
}

fn handle_proposal_create_upgrade<'a>(
    command: Command<'a>,
    non_confirm: bool,
    storage: &Storage,
) -> Result<io::CommandResponse<'a>, AppSW> {
    let request = parse_proposal_create_upgrade_request(command.get_data())?;
    let slot = storage
        .find_multisig(&request.multisig)
        .ok_or(AppSW::NotFound)?;
    let entry = storage.read_slot(slot).ok_or(AppSW::NotFound)?;

    let artifacts = build_proposal_create_upgrade_artifacts(
        &entry.member,
        &request.multisig,
        &entry.derivation_path,
        entry.path_length,
        request.transaction_index,
        request.vault_index,
        &request.program,
        &request.buffer,
        &request.spill,
        &request.transaction_blockhash,
        &request.proposal_blockhash,
    )?;

    let reviewed = ReviewedUpgradeIntent {
        occupied: true,
        multisig: request.multisig,
        transaction_index: request.transaction_index,
        vault_index: request.vault_index,
        program: request.program,
        buffer: request.buffer,
        spill: request.spill,
        intent_hash: artifacts.intent_hash,
    };

    let comm = command.into_comm();
    if !non_confirm
        && !show_create_upgrade_review(
            comm,
            &request.multisig,
            request.transaction_index,
            request.vault_index,
            &request.program,
            &request.buffer,
            &request.spill,
            &artifacts.intent_hash,
            &artifacts.create_message_hash,
            &artifacts.proposal_message_hash,
        )
    {
        return Err(AppSW::Deny);
    }

    if storage.upsert_reviewed_upgrade(&reviewed).is_none() {
        return Err(AppSW::ConditionsNotSatisfied);
    }

    if !non_confirm {
        show_status(comm, true);
    }

    let mut response = comm.begin_response();
    response.append(&artifacts.create_signature[..SIGNATURE_LENGTH])?;
    response.append(&artifacts.proposal_signature[..SIGNATURE_LENGTH])?;
    response.append(&artifacts.intent_hash[..MESSAGE_HASH_LENGTH])?;
    response.append(&artifacts.create_message_hash[..MESSAGE_HASH_LENGTH])?;
    response.append(&artifacts.proposal_message_hash[..MESSAGE_HASH_LENGTH])?;
    Ok(response)
}

fn handle_proposal_execute_upgrade<'a>(
    command: Command<'a>,
    non_confirm: bool,
    storage: &Storage,
) -> Result<io::CommandResponse<'a>, AppSW> {
    let request = parse_proposal_execute_upgrade_request(command.get_data())?;
    let slot = storage
        .find_multisig(&request.multisig)
        .ok_or(AppSW::NotFound)?;
    let entry = storage.read_slot(slot).ok_or(AppSW::NotFound)?;
    let reviewed_slot = storage
        .find_reviewed_upgrade(&request.multisig, request.transaction_index)
        .ok_or(AppSW::NotFound)?;
    let reviewed = storage
        .read_reviewed_upgrade(reviewed_slot)
        .ok_or(AppSW::NotFound)?;

    if reviewed.vault_index != request.vault_index
        || reviewed.program != request.program
        || reviewed.buffer != request.buffer
        || reviewed.spill != request.spill
    {
        return Err(AppSW::ConditionsNotSatisfied);
    }

    let artifacts = build_proposal_execute_upgrade_artifacts(ExecuteUpgradeInputs {
        member: &entry.member,
        multisig: &request.multisig,
        derivation_path: &entry.derivation_path,
        path_length: entry.path_length,
        transaction_index: request.transaction_index,
        vault_index: request.vault_index,
        program: &request.program,
        buffer: &request.buffer,
        spill: &request.spill,
        recent_blockhash: &request.blockhash,
    })?;

    if reviewed.intent_hash != artifacts.intent_hash {
        return Err(AppSW::ConditionsNotSatisfied);
    }

    let comm = command.into_comm();
    if !non_confirm
        && !show_execute_upgrade_review(
            comm,
            &request.multisig,
            request.transaction_index,
            request.vault_index,
            &request.program,
            &request.buffer,
            &request.spill,
            &artifacts.intent_hash,
            &artifacts.message_hash,
        )
    {
        return Err(AppSW::Deny);
    }

    if !non_confirm {
        show_status(comm, true);
    }

    let mut response = comm.begin_response();
    response.append(&artifacts.signature[..SIGNATURE_LENGTH])?;
    response.append(&artifacts.intent_hash[..MESSAGE_HASH_LENGTH])?;
    response.append(&artifacts.message_hash[..MESSAGE_HASH_LENGTH])?;
    Ok(response)
}

#[no_mangle]
extern "C" fn sample_main(_arg0: u32) {
    let comm = init_comm(&COMM);
    comm.set_expected_cla(APP_CLA);
    let mut home = ui_menu_main(comm);
    home.show_and_return();

    let storage = Storage;
    storage.init();

    loop {
        let command = comm.next_command();
        let decoded = command.decode::<Instruction>();
        let Ok(ins) = decoded else {
            let _ = comm.send(&[], decoded.unwrap_err());
            continue;
        };

        let status = match handle_apdu(command, &ins, &storage) {
            Ok(reply) => {
                let _ = reply.send(AppSW::Ok);
                AppSW::Ok
            }
            Err(sw) => {
                let _ = comm.send(&[], sw);
                sw
            }
        };

        if matches!(status, AppSW::Ok | AppSW::Deny) {
            home.show_and_return();
        }
    }
}

fn handle_apdu<'a>(
    command: Command<'a>,
    ins: &Instruction,
    storage: &Storage,
) -> Result<io::CommandResponse<'a>, AppSW> {
    match ins {
        Instruction::GetVersion => {
            let mut response = command.into_response();
            response.append(&[0, 1, 0])?;
            Ok(response)
        }
        Instruction::SaveMultisig { non_confirm } => {
            handle_save_multisig(command, *non_confirm, storage)
        }
        Instruction::ListMultisigSlot { slot } => handle_list_multisig_slot(command, *slot, storage),
        Instruction::ResetMultisigs { non_confirm } => {
            handle_reset_multisigs(command, *non_confirm, storage)
        }
        Instruction::ProposalVote { non_confirm } => {
            handle_proposal_vote(command, *non_confirm, storage)
        }
        Instruction::ProposalCreateUpgrade { non_confirm } => {
            handle_proposal_create_upgrade(command, *non_confirm, storage)
        }
        Instruction::ProposalExecuteUpgrade { non_confirm } => {
            handle_proposal_execute_upgrade(command, *non_confirm, storage)
        }
    }
}
