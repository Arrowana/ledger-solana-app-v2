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

use crate::AppSW;
use ledger_device_sdk::io::Comm;

#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
use ledger_device_sdk::ui::gadgets::{Field, MultiFieldReview};

#[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
use ledger_device_sdk::include_gif;
#[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
use ledger_device_sdk::nbgl::{NbglAddressReview, NbglGlyph};

#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
pub fn ui_display_address(_: &mut Comm, address: &str) -> Result<bool, AppSW> {
    let fields = [Field {
        name: "Address",
        value: address,
    }];

    Ok(MultiFieldReview::new(
        &fields,
        &["Verify", "Solana address"],
        None,
        "Approve",
        None,
        "Reject",
        None,
    )
    .show())
}

#[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
pub fn ui_display_address(comm: &mut Comm, address: &str) -> Result<bool, AppSW> {
    #[cfg(target_os = "apex_p")]
    const FERRIS: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_48x48.png", NBGL));
    #[cfg(any(target_os = "stax", target_os = "flex"))]
    const FERRIS: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_64x64.gif", NBGL));

    Ok(NbglAddressReview::new()
        .glyph(&FERRIS)
        .review_title("Verify Solana address")
        .show(comm, address))
}
