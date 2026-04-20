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
    pub mod menu;
    pub mod solana;
}
mod idls;
mod settings;
mod solana;

use app_ui::address::ui_display_address;
use app_ui::menu::ui_menu_main;
use app_ui::solana::{review_message, show_status};
use ledger_device_sdk::io::{self, init_comm, ApduHeader, Command, Reply, StatusWords};
use solana::{
    app_config_response, derive_pubkey, parse_derivation_path, parse_sign_payload, sign_message,
    SignMessageContext, APP_CLA, INS_GET_APP_CONFIG, INS_GET_PUBKEY, INS_SIGN_MESSAGE, P1_CONFIRM,
    P1_NON_CONFIRM, P2_EXTEND, P2_MORE,
};

ledger_device_sdk::set_panic!(ledger_device_sdk::exiting_panic);
ledger_device_sdk::define_comm!(COMM);

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

impl From<io::CommError> for AppSW {
    fn from(_: io::CommError) -> Self {
        AppSW::CommError
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Instruction {
    GetAppConfig,
    GetPubkey { display: bool },
    SignMessage { p2: u8 },
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
            (INS_GET_PUBKEY | INS_SIGN_MESSAGE, _, _) => Err(AppSW::WrongP1P2),
            (APP_CLA, _, _) => Err(AppSW::InsNotSupported),
            _ => Err(AppSW::InsNotSupported),
        }
    }
}

fn handle_get_app_config<'a>(command: Command<'a>) -> Result<io::CommandResponse<'a>, AppSW> {
    let config = app_config_response()?;
    let mut response = command.into_response();
    response.append(&config)?;
    Ok(response)
}

fn handle_get_pubkey<'a>(
    command: Command<'a>,
    display: bool,
) -> Result<io::CommandResponse<'a>, AppSW> {
    let (path, consumed) = parse_derivation_path(command.get_data())?;
    if consumed != command.get_data().len() {
        return Err(AppSW::WrongApduLength);
    }

    let pubkey = derive_pubkey(path.as_slice())?;
    let address = bs58::encode(pubkey).into_string();

    let comm = command.into_comm();
    if display && !ui_display_address(comm, address.as_str())? {
        return Err(AppSW::Deny);
    }

    let mut response = comm.begin_response();
    response.append(&pubkey)?;
    Ok(response)
}

fn handle_sign_message<'a>(
    command: Command<'a>,
    p2: u8,
    sign_context: &mut SignMessageContext,
) -> Result<io::CommandResponse<'a>, AppSW> {
    let completed = sign_context.ingest(p2, command.get_data())?;
    if !completed {
        return Ok(command.into_response());
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

    let comm = command.into_comm();
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
    let mut response = comm.begin_response();
    response.append(&signature)?;
    sign_context.reset();
    Ok(response)
}

#[no_mangle]
extern "C" fn sample_main(_arg0: u32) {
    let comm = init_comm(&COMM);
    comm.set_expected_cla(APP_CLA);
    let mut home = ui_menu_main(comm);
    home.show_and_return();

    let mut sign_context = SignMessageContext::new();

    loop {
        let command = comm.next_command();
        let decoded = command.decode::<Instruction>();
        let Ok(ins) = decoded else {
            sign_context.reset();
            let _ = comm.send(&[], decoded.unwrap_err());
            continue;
        };

        if !matches!(ins, Instruction::SignMessage { .. }) {
            sign_context.reset();
        }

        let status = match handle_apdu(command, ins, &mut sign_context) {
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
    ins: Instruction,
    sign_context: &mut SignMessageContext,
) -> Result<io::CommandResponse<'a>, AppSW> {
    match ins {
        Instruction::GetAppConfig => handle_get_app_config(command),
        Instruction::GetPubkey { display } => handle_get_pubkey(command, display),
        Instruction::SignMessage { p2 } => handle_sign_message(command, p2, sign_context),
    }
}
