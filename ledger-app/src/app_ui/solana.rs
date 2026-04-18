use alloc::{format, string::String};

use crate::AppSW;
use ledger_device_sdk::include_gif;
use ledger_device_sdk::io::Comm;
use ledger_device_sdk::nbgl::{
    Field, NbglGlyph, NbglReviewStatus, NbglStreamingReview, NbglStreamingReviewStatus, StatusType,
    TransactionType,
};
use solana_message_light::{
    AccountRefView, CompiledInstructionView, LookupAccountRefView, MessageVersion, MessageView,
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

pub fn review_message(signer_pubkey: &[u8; 32], message: &[u8]) -> Result<bool, AppSW> {
    let view = MessageView::try_new(message).map_err(|_| AppSW::InvalidData)?;
    let review = NbglStreamingReview::new()
        .tx_type(TransactionType::Transaction)
        .glyph(&APP_GLYPH);

    if !review.start("Review Solana tx", None) {
        return Ok(false);
    }

    let version = match view.version {
        MessageVersion::Legacy => "Legacy",
        MessageVersion::V0 => "V0",
    };
    let instruction_count = format!("{}", view.instruction_count());
    let account_count = format!("{}", view.total_account_count());
    if !push_page(
        &review,
        &[
            Field {
                name: "Version",
                value: version,
            },
            Field {
                name: "Instructions",
                value: instruction_count.as_str(),
            },
            Field {
                name: "Accounts",
                value: account_count.as_str(),
            },
        ],
    ) {
        return Ok(false);
    }

    let signer = format_base58(signer_pubkey);
    let fee_payer = format_base58(view.static_account(0).ok_or(AppSW::InvalidData)?);
    if !push_page(
        &review,
        &[
            Field {
                name: "Signer",
                value: signer.as_str(),
            },
            Field {
                name: "Fee payer",
                value: fee_payer.as_str(),
            },
        ],
    ) {
        return Ok(false);
    }

    let blockhash = format_base58(view.recent_blockhash());
    if !push_page(
        &review,
        &[Field {
            name: "Blockhash",
            value: blockhash.as_str(),
        }],
    ) {
        return Ok(false);
    }

    for instruction in view.instructions() {
        let instruction = instruction.map_err(|_| AppSW::InvalidData)?;
        if !review_instruction(&review, &view, instruction)? {
            return Ok(false);
        }
    }

    Ok(review.finish("Sign transaction"))
}

pub fn show_status<const N: usize>(comm: &mut Comm<N>, ok: bool) {
    NbglReviewStatus::new()
        .status_type(StatusType::Transaction)
        .show(comm, ok);
}

fn review_instruction(
    review: &NbglStreamingReview,
    view: &MessageView<'_>,
    instruction: CompiledInstructionView<'_>,
) -> Result<bool, AppSW> {
    let instruction_title = format!("{} / {}", instruction.index + 1, view.instruction_count());
    let program = match view
        .account_ref(instruction.program_id_index)
        .map_err(|_| AppSW::InvalidData)?
    {
        AccountRefView::Static(account) => format_base58(account.pubkey),
        AccountRefView::Lookup(_) => return Err(AppSW::InvalidData),
    };
    if !push_page(
        review,
        &[
            Field {
                name: "Instruction",
                value: instruction_title.as_str(),
            },
            Field {
                name: "Program",
                value: program.as_str(),
            },
        ],
    ) {
        return Ok(false);
    }

    let account_count = format!("{}", instruction.account_indexes.len());
    let data_len = format!("{} bytes", instruction.data.len());
    if !push_page(
        review,
        &[
            Field {
                name: "Ix accounts",
                value: account_count.as_str(),
            },
            Field {
                name: "Ix data",
                value: data_len.as_str(),
            },
        ],
    ) {
        return Ok(false);
    }

    for (position, account_index) in instruction.account_indexes.iter().enumerate() {
        let label = format!("Account {}", position + 1);
        let value = format_account_ref(
            view.account_ref(*account_index)
                .map_err(|_| AppSW::InvalidData)?,
        );
        if !push_page(
            review,
            &[Field {
                name: label.as_str(),
                value: value.as_str(),
            }],
        ) {
            return Ok(false);
        }
    }

    if instruction.data.is_empty() {
        return Ok(push_page(
            review,
            &[Field {
                name: "Data",
                value: "empty",
            }],
        ));
    }

    for (chunk_index, chunk) in instruction.data.chunks(IX_DATA_CHUNK_BYTES).enumerate() {
        let label = format!("Data {}", chunk_index + 1);
        let value = hex::encode(chunk);
        if !push_page(
            review,
            &[Field {
                name: label.as_str(),
                value: value.as_str(),
            }],
        ) {
            return Ok(false);
        }
    }

    Ok(true)
}

fn push_page(review: &NbglStreamingReview, fields: &[Field<'_>]) -> bool {
    matches!(review.next(fields), NbglStreamingReviewStatus::Next)
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
