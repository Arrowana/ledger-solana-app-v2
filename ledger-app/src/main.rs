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

extern crate alloc;

mod app_ui {
    pub mod address;
    pub mod idl_import;
    #[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
    pub mod idl_settings;
    pub mod menu;
    #[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
    pub mod review_scroller;
    pub mod solana;
}
mod idls;
mod settings;
mod solana;

use app_ui::address::ui_display_address;
use app_ui::idl_import::review_idl_import;
use app_ui::menu::ui_menu_main;
#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
use app_ui::menu::HomeAction;
use app_ui::solana::{review_message, show_status};
use idls::{prepare_idl_import, store_prepared_idl, verify_prepared_idl_import};
#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
use ledger_device_sdk::exit_app;
use ledger_device_sdk::io::{ApduHeader, Comm, Reply, StatusWords};
use solana::{
    app_config_response, derive_pubkey, parse_derivation_path, parse_sign_payload, sign_message,
    LoadIdlContext, SignMessageContext, APP_CLA, INS_GET_APP_CONFIG, INS_GET_PUBKEY, INS_LOAD_IDL,
    INS_SIGN_MESSAGE, P1_CONFIRM, P1_NON_CONFIRM, P2_EXTEND, P2_MORE,
};

ledger_device_sdk::set_panic!(ledger_device_sdk::exiting_panic);

#[repr(u16)]
#[derive(Clone, Copy, PartialEq)]
pub enum AppSW {
    Deny = 0x6985,
    ConditionsNotSatisfied = 0x6986,
    InvalidData = 0x6A80,
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

#[derive(Clone, Copy, Debug)]
pub enum Instruction {
    GetAppConfig,
    GetPubkey { display: bool },
    SignMessage { p2: u8 },
    LoadIdl { p2: u8 },
}

impl TryFrom<ApduHeader> for Instruction {
    type Error = AppSW;

    fn try_from(value: ApduHeader) -> Result<Self, Self::Error> {
        match (value.ins, value.p1, value.p2) {
            (INS_GET_APP_CONFIG, 0, 0) => Ok(Self::GetAppConfig),
            (INS_GET_PUBKEY, P1_NON_CONFIRM, 0) => Ok(Self::GetPubkey { display: false }),
            (INS_GET_PUBKEY, P1_CONFIRM, 0) => Ok(Self::GetPubkey { display: true }),
            (INS_SIGN_MESSAGE, P1_CONFIRM, p2) if p2 & !(P2_EXTEND | P2_MORE) == 0 => {
                Ok(Self::SignMessage { p2 })
            }
            (INS_LOAD_IDL, P1_NON_CONFIRM, p2) if p2 & !(P2_EXTEND | P2_MORE) == 0 => {
                Ok(Self::LoadIdl { p2 })
            }
            (INS_GET_PUBKEY | INS_SIGN_MESSAGE | INS_LOAD_IDL, _, _) => Err(AppSW::WrongP1P2),
            (APP_CLA, _, _) => Err(AppSW::InsNotSupported),
            _ => Err(AppSW::InsNotSupported),
        }
    }
}

fn handle_get_app_config(comm: &mut Comm) -> Result<(), AppSW> {
    let config = app_config_response()?;
    comm.append(&config);
    Ok(())
}

fn handle_get_pubkey(comm: &mut Comm, display: bool) -> Result<(), AppSW> {
    let data = comm.get_data().map_err(|_| AppSW::WrongApduLength)?;
    let (path, consumed) = parse_derivation_path(data)?;
    if consumed != data.len() {
        return Err(AppSW::WrongApduLength);
    }

    let pubkey = derive_pubkey(path.as_slice())?;
    let address = bs58::encode(pubkey).into_string();

    if display && !ui_display_address(comm, address.as_str())? {
        return Err(AppSW::Deny);
    }

    comm.append(&pubkey);
    Ok(())
}

fn handle_sign_message(
    comm: &mut Comm,
    p2: u8,
    sign_context: &mut SignMessageContext,
) -> Result<(), AppSW> {
    let completed =
        sign_context.ingest(p2, comm.get_data().map_err(|_| AppSW::WrongApduLength)?)?;
    if !completed {
        return Ok(());
    }

    let sign_payload = match parse_sign_payload(sign_context.payload()) {
        Ok(payload) => payload,
        Err(error) => {
            sign_context.reset();
            return Err(error);
        }
    };

    let signer_pubkey = match derive_pubkey(sign_payload.path.as_slice()) {
        Ok(pubkey) => pubkey,
        Err(error) => {
            sign_context.reset();
            return Err(error);
        }
    };

    if !review_message(comm, &signer_pubkey, sign_payload.message)? {
        sign_context.reset();
        return Err(AppSW::Deny);
    }

    let signature = match sign_message(sign_payload.path.as_slice(), sign_payload.message) {
        Ok(signature) => signature,
        Err(error) => {
            sign_context.reset();
            return Err(error);
        }
    };

    show_status(comm, true);
    comm.append(&signature);
    sign_context.reset();
    Ok(())
}

fn handle_load_idl(
    comm: &mut Comm,
    p2: u8,
    load_idl_context: &mut LoadIdlContext,
) -> Result<(), AppSW> {
    let completed =
        load_idl_context.ingest(p2, comm.get_data().map_err(|_| AppSW::WrongApduLength)?)?;
    if !completed {
        return Ok(());
    }

    let prepared = match prepare_idl_import(load_idl_context.payload()) {
        Ok(prepared) => prepared,
        Err(error) => {
            load_idl_context.reset();
            return Err(error);
        }
    };

    if let Err(error) = verify_prepared_idl_import(&prepared) {
        load_idl_context.reset();
        return Err(error);
    }

    if !review_idl_import(
        comm,
        &prepared.program_id,
        prepared.signer_pubkeys.as_slice(),
    )? {
        load_idl_context.reset();
        return Err(AppSW::Deny);
    }

    let response = store_prepared_idl(&prepared);

    comm.append(&response.program_id);
    comm.append(&[response.signer_count]);
    comm.append(&response.idl_len.to_be_bytes());
    load_idl_context.reset();
    Ok(())
}

#[no_mangle]
extern "C" fn sample_main(_arg0: u32) {
    let mut comm = Comm::new().set_expected_cla(APP_CLA);

    let mut sign_context = SignMessageContext::new();
    let mut load_idl_context = LoadIdlContext::new();

    #[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
    loop {
        let ins = match ui_menu_main(&mut comm).show_and_wait() {
            HomeAction::Command(instruction) => instruction,
            HomeAction::Quit => exit_app(0),
        };

        if !matches!(ins, Instruction::SignMessage { .. }) {
            sign_context.reset();
        }
        if !matches!(ins, Instruction::LoadIdl { .. }) {
            load_idl_context.reset();
        }

        let status = match handle_apdu(&mut comm, ins, &mut sign_context, &mut load_idl_context) {
            Ok(()) => AppSW::Ok,
            Err(sw) => sw,
        };
        comm.reply(status);
    }

    #[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
    {
        let mut home = ui_menu_main(&mut comm);
        home.show_and_return();

        loop {
            let ins = comm.next_command::<Instruction>();

            if !matches!(ins, Instruction::SignMessage { .. }) {
                sign_context.reset();
            }
            if !matches!(ins, Instruction::LoadIdl { .. }) {
                load_idl_context.reset();
            }

            let status = match handle_apdu(&mut comm, ins, &mut sign_context, &mut load_idl_context)
            {
                Ok(()) => AppSW::Ok,
                Err(sw) => sw,
            };
            comm.reply(status);

            if matches!(status, AppSW::Ok | AppSW::Deny) {
                home.show_and_return();
            }
        }
    }
}

fn handle_apdu(
    comm: &mut Comm,
    ins: Instruction,
    sign_context: &mut SignMessageContext,
    load_idl_context: &mut LoadIdlContext,
) -> Result<(), AppSW> {
    match ins {
        Instruction::GetAppConfig => handle_get_app_config(comm),
        Instruction::GetPubkey { display } => handle_get_pubkey(comm, display),
        Instruction::SignMessage { p2 } => handle_sign_message(comm, p2, sign_context),
        Instruction::LoadIdl { p2 } => handle_load_idl(comm, p2, load_idl_context),
    }
}
