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

use ledger_device_sdk::io::Comm;

#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
use ledger_device_sdk::ui::{
    gadgets::clear_screen,
    layout::{Layout, Location, StringPlace},
    screen_util::screen_update,
};

#[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
use ledger_device_sdk::include_gif;
#[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
use ledger_device_sdk::nbgl::{NbglGlyph, NbglHomeAndSettings};

#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
pub struct HomeScreen;

#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
impl HomeScreen {
    pub fn show_and_return(&mut self) {
        clear_screen();
        ["Solana v2", "app is ready"].place(Location::Middle, Layout::Centered, false);
        screen_update();
    }
}

#[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
pub type HomeScreen = NbglHomeAndSettings;

#[cfg(any(target_os = "nanosplus", target_os = "nanox"))]
pub fn ui_menu_main(_: &mut Comm) -> HomeScreen {
    HomeScreen
}

#[cfg(not(any(target_os = "nanosplus", target_os = "nanox")))]
pub fn ui_menu_main(_: &mut Comm) -> HomeScreen {
    #[cfg(target_os = "apex_p")]
    const FERRIS: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_48x48.png", NBGL));
    #[cfg(any(target_os = "stax", target_os = "flex"))]
    const FERRIS: NbglGlyph = NbglGlyph::from_include(include_gif!("glyphs/crab_64x64.gif", NBGL));

    let settings_strings: [[&str; 2]; 0] = [];
    let mut settings = crate::settings::Settings::default();

    NbglHomeAndSettings::new()
        .glyph(&FERRIS)
        .settings(settings.get_mut(), &settings_strings)
        .infos("Solana v2", env!("CARGO_PKG_VERSION"), "OpenAI")
}
