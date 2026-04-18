use ledger_device_sdk::include_gif;
use ledger_device_sdk::io::Comm;
use ledger_device_sdk::nbgl::{
    Field, NbglChoice, NbglGlyph, NbglReview, NbglReviewStatus, StatusType, TransactionType,
};
use numtoa::NumToA;

use crate::storage::SavedMultisigEntry;

#[cfg(target_os = "apex_p")]
const APP_GLYPH: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_48x48.png", NBGL));
#[cfg(any(target_os = "stax", target_os = "flex"))]
const APP_GLYPH: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_64x64.gif", NBGL));
#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
const APP_GLYPH: NbglGlyph = NbglGlyph::from_include(include_gif!("icons/crab_14x14.gif", NBGL));

pub fn show_save_review<const N: usize>(comm: &mut Comm<N>, entry: &SavedMultisigEntry) -> bool {
    let multisig = short_key(&entry.multisig);
    let member = short_key(&entry.member);
    let path = format_path(entry);
    let fields = [
        Field {
            name: "Action",
            value: "Save multisig",
        },
        Field {
            name: "Multisig",
            value: &multisig,
        },
        Field {
            name: "Member",
            value: &member,
        },
        Field {
            name: "Path",
            value: &path,
        },
    ];

    NbglReview::new()
        .light()
        .tx_type(TransactionType::Transaction)
        .glyph(&APP_GLYPH)
        .titles("Review multisig", "", "Approve")
        .show(comm, &fields)
}

pub fn show_reset_review<const N: usize>(comm: &mut Comm<N>) -> bool {
    NbglChoice::new().glyph(&APP_GLYPH).show(
        comm,
        "Reset multisigs?",
        "This deletes all saved bindings.",
        "Reset",
        "Cancel",
    )
}

pub fn show_vote_review<const N: usize>(
    comm: &mut Comm<N>,
    multisig: &[u8; 32],
    member: &[u8; 32],
    transaction_index: u64,
    vote: u8,
    message_hash: &[u8; 32],
) -> bool {
    let action = if vote == 0x00 {
        "Approve vote"
    } else {
        "Reject vote"
    };
    let multisig = short_key(multisig);
    let member = short_key(member);
    let transaction_index = format_u64(transaction_index);
    let message_hash = format_hex(message_hash);
    let fields = [
        Field {
            name: "Action",
            value: action,
        },
        Field {
            name: "Multisig",
            value: &multisig,
        },
        Field {
            name: "Member",
            value: &member,
        },
        Field {
            name: "Tx index",
            value: &transaction_index,
        },
        Field {
            name: "Message hash",
            value: &message_hash,
        },
    ];

    NbglReview::new()
        .light()
        .tx_type(TransactionType::Transaction)
        .glyph(&APP_GLYPH)
        .titles("Review proposal vote", "", "Approve")
        .show(comm, &fields)
}

pub fn show_create_upgrade_review<const N: usize>(
    comm: &mut Comm<N>,
    multisig: &[u8; 32],
    transaction_index: u64,
    vault_index: u8,
    program: &[u8; 32],
    buffer: &[u8; 32],
    spill: &[u8; 32],
    intent_hash: &[u8; 32],
    create_hash: &[u8; 32],
    proposal_hash: &[u8; 32],
) -> bool {
    let multisig = short_key(multisig);
    let transaction_index = format_u64(transaction_index);
    let vault_index = format_u8(vault_index);
    let program = short_key(program);
    let buffer = short_key(buffer);
    let spill = short_key(spill);
    let intent_hash = format_hex(intent_hash);
    let create_hash = format_hex(create_hash);
    let proposal_hash = format_hex(proposal_hash);
    let fields = [
        Field {
            name: "Multisig",
            value: &multisig,
        },
        Field {
            name: "Tx index",
            value: &transaction_index,
        },
        Field {
            name: "Vault",
            value: &vault_index,
        },
        Field {
            name: "Program",
            value: &program,
        },
        Field {
            name: "Buffer",
            value: &buffer,
        },
        Field {
            name: "Spill",
            value: &spill,
        },
        Field {
            name: "Intent hash",
            value: &intent_hash,
        },
        Field {
            name: "Create hash",
            value: &create_hash,
        },
        Field {
            name: "Proposal hash",
            value: &proposal_hash,
        },
    ];

    NbglReview::new()
        .light()
        .tx_type(TransactionType::Transaction)
        .glyph(&APP_GLYPH)
        .titles("Create upgrade", "", "Approve")
        .show(comm, &fields)
}

pub fn show_execute_upgrade_review<const N: usize>(
    comm: &mut Comm<N>,
    multisig: &[u8; 32],
    transaction_index: u64,
    vault_index: u8,
    program: &[u8; 32],
    buffer: &[u8; 32],
    spill: &[u8; 32],
    intent_hash: &[u8; 32],
    message_hash: &[u8; 32],
) -> bool {
    let multisig = short_key(multisig);
    let transaction_index = format_u64(transaction_index);
    let vault_index = format_u8(vault_index);
    let program = short_key(program);
    let buffer = short_key(buffer);
    let spill = short_key(spill);
    let intent_hash = format_hex(intent_hash);
    let message_hash = format_hex(message_hash);
    let fields = [
        Field {
            name: "Multisig",
            value: &multisig,
        },
        Field {
            name: "Tx index",
            value: &transaction_index,
        },
        Field {
            name: "Vault",
            value: &vault_index,
        },
        Field {
            name: "Program",
            value: &program,
        },
        Field {
            name: "Buffer",
            value: &buffer,
        },
        Field {
            name: "Spill",
            value: &spill,
        },
        Field {
            name: "Intent hash",
            value: &intent_hash,
        },
        Field {
            name: "Execute hash",
            value: &message_hash,
        },
    ];

    NbglReview::new()
        .light()
        .tx_type(TransactionType::Transaction)
        .glyph(&APP_GLYPH)
        .titles("Execute upgrade", "", "Approve")
        .show(comm, &fields)
}

pub fn show_status<const N: usize>(comm: &mut Comm<N>, ok: bool) {
    NbglReviewStatus::new()
        .status_type(StatusType::Transaction)
        .show(comm, ok);
}

fn short_key(bytes: &[u8; 32]) -> heapless::String<17> {
    let mut out = heapless::String::<17>::new();
    let _ = core::fmt::write(
        &mut out,
        format_args!(
            "{:02x}{:02x}..{:02x}{:02x}",
            bytes[0], bytes[1], bytes[30], bytes[31]
        ),
    );
    out
}

fn format_path(entry: &SavedMultisigEntry) -> heapless::String<48> {
    let mut out = heapless::String::<48>::new();
    let _ = out.push_str("m");
    for segment in entry
        .derivation_path
        .iter()
        .take(entry.path_length as usize)
        .copied()
    {
        let hardened = (segment & 0x8000_0000) != 0;
        let value = segment & 0x7fff_ffff;
        let _ = core::fmt::write(&mut out, format_args!("/{}", value));
        if hardened {
            let _ = out.push('\'');
        }
    }
    out
}

fn format_u64(value: u64) -> heapless::String<21> {
    let mut bytes = [0u8; 20];
    let rendered = value.numtoa_str(10, &mut bytes);
    let mut out = heapless::String::<21>::new();
    let _ = out.push_str(rendered);
    out
}

fn format_u8(value: u8) -> heapless::String<4> {
    let mut bytes = [0u8; 3];
    let rendered = value.numtoa_str(10, &mut bytes);
    let mut out = heapless::String::<4>::new();
    let _ = out.push_str(rendered);
    out
}

fn format_hex(bytes: &[u8; 32]) -> heapless::String<64> {
    let mut out = heapless::String::<64>::new();
    for byte in bytes {
        let _ = core::fmt::write(&mut out, format_args!("{:02x}", byte));
    }
    out
}
