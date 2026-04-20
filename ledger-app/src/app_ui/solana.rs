use alloc::{format, string::String, vec::Vec};

use crate::{
    idls::{decode_instruction as decode_builtin_instruction, load_builtin_idls, LoadedBuiltinIdl},
    AppSW,
};
use codama_parser::{DecodedField, DecodedInstruction, DecodedNumber, DecodedValue};
use ledger_device_sdk::include_gif;
use ledger_device_sdk::io::Comm;
use ledger_device_sdk::nbgl::{
    Field, NbglGlyph, NbglReview, NbglReviewStatus, StatusType, TransactionType,
};
use solana_message_light::{
    AccountRefView, CompiledInstructionView, LookupAccountRefView, MessageView,
    StaticAccountRefView,
};

#[cfg(target_os = "apex_p")]
const APP_GLYPH: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_48x48.png", NBGL));
#[cfg(any(target_os = "stax", target_os = "flex"))]
const APP_GLYPH: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_64x64.gif", NBGL));
#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
const APP_GLYPH: NbglGlyph =
    NbglGlyph::from_include(include_gif!("glyphs/home_nano_nbgl.png", NBGL));

const IX_DATA_CHUNK_BYTES: usize = 16;

struct OwnedField {
    name: String,
    value: String,
}

pub fn review_message<const N: usize>(
    comm: &mut Comm<N>,
    _signer_pubkey: &[u8; 32],
    message: &[u8],
) -> Result<bool, AppSW> {
    let view = MessageView::try_new(message).map_err(|_| AppSW::InvalidData)?;
    let builtin_idls = load_builtin_idls();
    let mut owned_fields = Vec::new();

    for instruction in view.instructions() {
        let instruction = instruction.map_err(|_| AppSW::InvalidData)?;
        review_instruction(
            &mut owned_fields,
            &view,
            instruction,
            builtin_idls.as_slice(),
        )?;
    }

    let rendered_fields: Vec<Field<'_>> = owned_fields
        .iter()
        .map(|field| Field {
            name: field.name.as_str(),
            value: field.value.as_str(),
        })
        .collect();

    Ok(NbglReview::new()
        .tx_type(TransactionType::Transaction)
        .glyph(&APP_GLYPH)
        .light()
        .titles("Review Solana tx", "", "Sign transaction")
        .show(comm, rendered_fields.as_slice()))
}

pub fn show_status<const N: usize>(comm: &mut Comm<N>, ok: bool) {
    NbglReviewStatus::new()
        .status_type(StatusType::Transaction)
        .show(comm, ok);
}

fn review_instruction(
    fields: &mut Vec<OwnedField>,
    view: &MessageView<'_>,
    instruction: CompiledInstructionView<'_>,
    builtin_idls: &[LoadedBuiltinIdl],
) -> Result<(), AppSW> {
    let instruction_title = format!("{} / {}", instruction.index + 1, view.instruction_count());
    let program_ref = view
        .account_ref(instruction.program_id_index)
        .map_err(|_| AppSW::InvalidData)?;
    let (program, decoded) = match program_ref {
        AccountRefView::Static(account) => (
            format_base58(account.pubkey),
            decode_builtin_instruction(builtin_idls, account.pubkey, instruction.data),
        ),
        AccountRefView::Lookup(account) => (format_lookup_account(account), None),
    };
    let program_label = decoded
        .as_ref()
        .map(|decoded| decoded.program_name)
        .unwrap_or(program.as_str());
    if let Some(decoded) = decoded.as_ref() {
        let instruction_summary = format!("{program_label}: {}", decoded.instruction.name);
        fields.push(OwnedField {
            name: String::from("Ix"),
            value: instruction_title,
        });
        fields.push(OwnedField {
            name: String::from("Instruction"),
            value: instruction_summary,
        });
        review_decoded_instruction(fields, &decoded.instruction);
    } else {
        let data_len = format!("{} bytes", instruction.data.len());
        fields.push(OwnedField {
            name: String::from("Ix"),
            value: instruction_title,
        });
        fields.push(OwnedField {
            name: String::from("Program"),
            value: String::from(program_label),
        });
        fields.push(OwnedField {
            name: String::from("Data"),
            value: data_len,
        });
    }

    for (position, account_index) in instruction.account_indexes.iter().enumerate() {
        let label = decoded
            .as_ref()
            .and_then(|decoded| decoded.instruction.account_names.get(position))
            .cloned()
            .unwrap_or_else(|| format!("Account {}", position + 1));
        let value = format_account_ref(
            view.account_ref(*account_index)
                .map_err(|_| AppSW::InvalidData)?,
        );
        fields.push(OwnedField { name: label, value });
    }

    if decoded.is_some() {
        return Ok(());
    }

    if instruction.data.is_empty() {
        fields.push(OwnedField {
            name: String::from("Data"),
            value: String::from("empty"),
        });
        return Ok(());
    }

    for (chunk_index, chunk) in instruction.data.chunks(IX_DATA_CHUNK_BYTES).enumerate() {
        let label = format!("Data {}", chunk_index + 1);
        let value = hex::encode(chunk);
        fields.push(OwnedField { name: label, value });
    }

    Ok(())
}

fn review_decoded_instruction(fields: &mut Vec<OwnedField>, instruction: &DecodedInstruction) {
    if instruction.arguments.is_empty() {
        fields.push(OwnedField {
            name: String::from("Arguments"),
            value: String::from("none"),
        });
        return;
    }

    for field in &instruction.arguments {
        collect_decoded_field(fields, field.name.as_str(), &field.value);
    }
}

fn collect_decoded_field(flattened: &mut Vec<OwnedField>, label: &str, value: &DecodedValue) {
    match value {
        DecodedValue::Number(number) => {
            flattened.push(OwnedField {
                name: String::from(label),
                value: render_number(number),
            });
        }
        DecodedValue::Boolean(value) => {
            flattened.push(OwnedField {
                name: String::from(label),
                value: String::from(if *value { "true" } else { "false" }),
            });
        }
        DecodedValue::PublicKey(value) => {
            flattened.push(OwnedField {
                name: String::from(label),
                value: format_base58(value),
            });
        }
        DecodedValue::Bytes(value) => {
            flattened.push(OwnedField {
                name: String::from(label),
                value: hex::encode(value),
            });
        }
        DecodedValue::String(value) => {
            flattened.push(OwnedField {
                name: String::from(label),
                value: value.clone(),
            });
        }
        DecodedValue::Option(value) => match value {
            Some(value) => collect_decoded_field(flattened, label, value),
            None => flattened.push(OwnedField {
                name: String::from(label),
                value: String::from("none"),
            }),
        },
        DecodedValue::Array(values) => {
            if values.is_empty() {
                flattened.push(OwnedField {
                    name: String::from(label),
                    value: String::from("[]"),
                });
                return;
            }

            for (index, value) in values.iter().enumerate() {
                let nested = format!("{}[{}]", label, index + 1);
                collect_decoded_field(flattened, nested.as_str(), value);
            }
        }
        DecodedValue::Struct(fields) => {
            if fields.is_empty() {
                flattened.push(OwnedField {
                    name: String::from(label),
                    value: String::from("{}"),
                });
                return;
            }

            for field in fields {
                let nested = format!("{}.{}", label, field.name);
                collect_decoded_field(flattened, nested.as_str(), &field.value);
            }
        }
        DecodedValue::Enum(variant) => {
            flattened.push(OwnedField {
                name: String::from(label),
                value: variant.name.clone(),
            });

            if let Some(fields) = &variant.value {
                for DecodedField { name, value } in fields {
                    let nested = format!("{}.{}", label, name);
                    collect_decoded_field(flattened, nested.as_str(), value);
                }
            }
        }
    }
}

fn render_number(number: &DecodedNumber) -> String {
    match number {
        DecodedNumber::U8(value) => format!("{value}"),
        DecodedNumber::U16(value) => format!("{value}"),
        DecodedNumber::U32(value) => format!("{value}"),
        DecodedNumber::U64(value) => format!("{value}"),
        DecodedNumber::I64(value) => format!("{value}"),
    }
}

fn format_account_ref(account: AccountRefView<'_>) -> String {
    match account {
        AccountRefView::Static(static_account) => format_static_account(static_account),
        AccountRefView::Lookup(lookup_account) => format_lookup_account(lookup_account),
    }
}

fn format_static_account(account: StaticAccountRefView<'_>) -> String {
    let mut out = format_base58(account.pubkey);
    if account.signer || account.writable {
        out.push_str(" [");
        if account.signer {
            out.push('S');
        }
        if account.writable {
            if account.signer {
                out.push(',');
            }
            out.push('W');
        }
        out.push(']');
    }
    out
}

fn format_lookup_account(account: LookupAccountRefView<'_>) -> String {
    let access = if account.writable { "w" } else { "r" };
    let table = format_base58(account.table_account);
    format!("ALT {}[{}] {}", access, account.table_index, table)
}

fn format_base58(bytes: &[u8]) -> String {
    bs58::encode(bytes).into_string()
}
