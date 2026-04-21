use alloc::{format, string::String, vec::Vec};

use crate::AppSW;
use ledger_device_sdk::io::Comm;

#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
use ledger_device_sdk::ui::gadgets::{Field, MultiFieldReview};

#[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
use ledger_device_sdk::nbgl::{Field, NbglReview, TransactionType};

struct OwnedField {
    name: String,
    value: String,
}

pub fn review_idl_import(
    comm: &mut Comm,
    program_id: &[u8; 32],
    signer_pubkeys: &[[u8; 32]],
) -> Result<bool, AppSW> {
    let owned_fields = build_fields(program_id, signer_pubkeys);
    let rendered_fields: Vec<Field<'_>> = owned_fields
        .iter()
        .map(|field| Field {
            name: field.name.as_str(),
            value: field.value.as_str(),
        })
        .collect();

    #[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
    {
        let _ = comm;
        return Ok(MultiFieldReview::new(
            rendered_fields.as_slice(),
            &["Review", "IDL import"],
            None,
            "Import",
            None,
            "Reject",
            None,
        )
        .show());
    }

    #[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
    {
        return Ok(NbglReview::new()
            .tx_type(TransactionType::Transaction)
            .light()
            .titles("Review IDL import", "", "Import IDL")
            .show(comm, rendered_fields.as_slice()));
    }
}

fn build_fields(program_id: &[u8; 32], signer_pubkeys: &[[u8; 32]]) -> Vec<OwnedField> {
    let mut fields = Vec::with_capacity(1 + signer_pubkeys.len());
    fields.push(OwnedField {
        name: String::from("programId"),
        value: format_base58(program_id),
    });
    for (index, signer_pubkey) in signer_pubkeys.iter().enumerate() {
        fields.push(OwnedField {
            name: format!("signer{}", index + 1),
            value: format_base58(signer_pubkey),
        });
    }
    fields
}

fn format_base58(bytes: &[u8; 32]) -> String {
    bs58::encode(bytes).into_string()
}
