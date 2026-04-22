use alloc::{string::String, vec::Vec};

use crate::idls::{clear_imported_idl, load_idls, LoadedIdl, LoadedIdlSource};
use ledger_device_sdk::ui::gadgets::{popup, Field, Menu, MultiFieldReview, Validator};

struct OwnedField {
    name: String,
    value: String,
}

pub fn show_settings() {
    loop {
        let loaded_idls = load_idls();
        let mut labels: Vec<String> = loaded_idls
            .iter()
            .map(|loaded_idl| loaded_idl.name.clone())
            .collect();
        let remove_index = labels.len();
        labels.push(String::from("Remove import"));
        let back_index = labels.len();
        labels.push(String::from("Back"));

        let panels: Vec<&str> = labels.iter().map(|label| label.as_str()).collect();
        match Menu::new(panels.as_slice()).show() {
            index if index < loaded_idls.len() => {
                let _ = show_idl_details(&loaded_idls[index]);
            }
            index if index == remove_index => remove_imported_idl(),
            index if index == back_index => return,
            _ => return,
        }
    }
}

fn remove_imported_idl() {
    let loaded_idls = load_idls();
    if loaded_idls
        .iter()
        .find(|loaded_idl| loaded_idl.source == LoadedIdlSource::Imported)
        .is_none()
    {
        popup("No import");
        return;
    }

    if Validator::new("Remove IDL").ask() {
        clear_imported_idl();
        popup("IDL removed");
    }
}

fn show_idl_details(loaded_idl: &LoadedIdl) -> bool {
    let owned_fields = build_fields(loaded_idl);
    let rendered_fields: Vec<Field<'_>> = owned_fields
        .iter()
        .map(|field| Field {
            name: field.name.as_str(),
            value: field.value.as_str(),
        })
        .collect();

    MultiFieldReview::new(
        rendered_fields.as_slice(),
        &["Loaded", "IDL"],
        None,
        "Done",
        None,
        "Back",
        None,
    )
    .show()
}

fn build_fields(loaded_idl: &LoadedIdl) -> Vec<OwnedField> {
    let mut fields = Vec::with_capacity(3);
    fields.push(OwnedField {
        name: String::from("name"),
        value: loaded_idl.name.clone(),
    });
    fields.push(OwnedField {
        name: String::from("source"),
        value: match loaded_idl.source {
            LoadedIdlSource::Builtin => String::from("builtin"),
            LoadedIdlSource::Imported => String::from("imported"),
        },
    });
    fields.push(OwnedField {
        name: String::from("removable"),
        value: match loaded_idl.source {
            LoadedIdlSource::Builtin => String::from("no (builtin)"),
            LoadedIdlSource::Imported => String::from("yes"),
        },
    });
    fields.push(OwnedField {
        name: String::from("programId"),
        value: bs58::encode(loaded_idl.program_id).into_string(),
    });
    fields
}
