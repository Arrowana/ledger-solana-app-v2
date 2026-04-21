use alloc::{format, string::String, vec::Vec};
use core::ffi::c_void;

use crate::{
    idls::{decode_instruction as decode_builtin_instruction, load_builtin_idls, LoadedBuiltinIdl},
    AppSW,
};
use codama_parser::{DecodedField, DecodedInstruction, DecodedNumber, DecodedValue};
use ledger_device_sdk::{
    hash::{sha2::Sha2_256, HashInit},
    io::Comm,
    ui::{
        bagls::{Icon, RectFull, DOWN_S_ARROW, UP_S_ARROW},
        fonts::OPEN_SANS,
        gadgets::{clear_screen, get_event, Validator},
        layout::Draw,
        screen_util::{draw, screen_update},
        SCREEN_HEIGHT, SCREEN_WIDTH,
    },
};
use ledger_secure_sdk_sys::buttons::ButtonsState;
use solana_message_light::{
    AccountRefView, CompiledInstructionView, LookupAccountRefView, MessageView,
    StaticAccountRefView,
};

const LINE_HEIGHT: usize = 12;
const TOP_PADDING: usize = 1;
const HORIZONTAL_PADDING: usize = 2;
const VALUE_INDENT_PIXELS: usize = 10;
const MESSAGE_HASH_LENGTH: usize = 32;
const FOOTER_HEIGHT: usize = LINE_HEIGHT;
const FOOTER_TEXT_Y: usize = SCREEN_HEIGHT - FOOTER_HEIGHT;

struct ReviewSection {
    lines: Vec<ReviewLine>,
}

struct ReviewLine {
    text: String,
    style: ReviewLineStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReviewLineStyle {
    Header,
    Label,
    Value,
}

pub fn review_message(
    _comm: &mut Comm,
    signer_pubkey: &[u8; 32],
    message: &[u8],
) -> Result<bool, AppSW> {
    let sections = build_sections(signer_pubkey, message)?;
    show_review(sections.as_slice())
}

fn build_sections(signer_pubkey: &[u8; 32], message: &[u8]) -> Result<Vec<ReviewSection>, AppSW> {
    let view = MessageView::try_new(message).map_err(|_| AppSW::InvalidData)?;
    let builtin_idls = load_builtin_idls();
    let mut sections = Vec::new();

    for instruction in view.instructions() {
        let instruction = instruction.map_err(|_| AppSW::InvalidData)?;
        sections.push(build_instruction_section(
            &view,
            instruction,
            builtin_idls.as_slice(),
            signer_pubkey,
        )?);
    }

    sections.push(build_final_section(&view, signer_pubkey, message)?);
    Ok(sections)
}

fn build_instruction_section(
    view: &MessageView<'_>,
    instruction: CompiledInstructionView<'_>,
    builtin_idls: &[LoadedBuiltinIdl],
    signer_pubkey: &[u8; 32],
) -> Result<ReviewSection, AppSW> {
    let instruction_title = format!("{}/{}", instruction.index + 1, view.instruction_count());
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

    let mut lines = Vec::new();
    if let Some(decoded) = decoded.as_ref() {
        push_instruction_header_lines(
            &mut lines,
            instruction_title.as_str(),
            decoded.program_name,
            decoded.instruction.name.as_str(),
        );
    } else {
        push_header_lines(
            &mut lines,
            format!("{instruction_title} {program}").as_str(),
        );
    }

    if let Some(decoded) = decoded.as_ref() {
        collect_decoded_instruction_lines(
            &mut lines,
            &decoded.instruction,
            view,
            instruction,
            signer_pubkey,
        )?;
    } else {
        if instruction.data.is_empty() {
            push_named_value_lines(&mut lines, "data", "empty");
        } else {
            push_named_value_lines(
                &mut lines,
                "dataLen",
                format!("{} bytes", instruction.data.len()).as_str(),
            );
        }

        for (position, account_index) in instruction.account_indexes.iter().enumerate() {
            let label = format!("account{}", position + 1);
            let value = format_account_ref(
                view.account_ref(*account_index)
                    .map_err(|_| AppSW::InvalidData)?,
                signer_pubkey,
            );
            push_named_value_lines(&mut lines, label.as_str(), value.as_str());
        }

        for (chunk_index, chunk) in instruction.data.chunks(16).enumerate() {
            let label = format!("data{}", chunk_index + 1);
            let value = hex::encode(chunk);
            push_named_value_lines(&mut lines, label.as_str(), value.as_str());
        }
    }

    Ok(ReviewSection { lines })
}

fn build_final_section(
    view: &MessageView<'_>,
    signer_pubkey: &[u8; 32],
    message: &[u8],
) -> Result<ReviewSection, AppSW> {
    let mut lines = Vec::new();

    push_header_lines(&mut lines, "FINAL REVIEW");

    push_inline_value_lines(
        &mut lines,
        format!(
            "ixs: {}",
            format_grouped_u64(view.instruction_count() as u64)
        )
        .as_str(),
    );

    if let Some(fee_payer) = view.static_account(0) {
        let fee_payer_value = if fee_payer == signer_pubkey {
            String::from("<wallet>")
        } else {
            format_base58(fee_payer)
        };
        push_named_value_lines(&mut lines, "feePayer", fee_payer_value.as_str());
    }

    push_named_value_lines(
        &mut lines,
        "Message SHA-256",
        format_message_hash(message)?.as_str(),
    );

    Ok(ReviewSection { lines })
}

fn collect_decoded_instruction_lines(
    lines: &mut Vec<ReviewLine>,
    instruction: &DecodedInstruction,
    view: &MessageView<'_>,
    compiled_instruction: CompiledInstructionView<'_>,
    signer_pubkey: &[u8; 32],
) -> Result<(), AppSW> {
    for field in &instruction.arguments {
        collect_decoded_field_lines(lines, field.name.as_str(), &field.value, signer_pubkey);
    }

    for (position, account_index) in compiled_instruction.account_indexes.iter().enumerate() {
        let label = instruction
            .account_names
            .get(position)
            .cloned()
            .unwrap_or_else(|| format!("account{}", position + 1));
        if label == "feePayer" {
            continue;
        }
        let value = format_account_ref(
            view.account_ref(*account_index)
                .map_err(|_| AppSW::InvalidData)?,
            signer_pubkey,
        );
        push_named_value_lines(lines, label.as_str(), value.as_str());
    }

    Ok(())
}

fn collect_decoded_field_lines(
    flattened: &mut Vec<ReviewLine>,
    label: &str,
    value: &DecodedValue,
    signer_pubkey: &[u8; 32],
) {
    match value {
        DecodedValue::Number(number) => {
            push_named_value_lines(flattened, label, render_number(number).as_str());
        }
        DecodedValue::Boolean(value) => {
            push_named_value_lines(flattened, label, if *value { "true" } else { "false" });
        }
        DecodedValue::PublicKey(value) => {
            let rendered = if value == signer_pubkey {
                String::from("<wallet>")
            } else {
                format_base58(value)
            };
            push_named_value_lines(flattened, label, rendered.as_str());
        }
        DecodedValue::Bytes(value) => {
            let rendered = hex::encode(value);
            push_named_value_lines(flattened, label, rendered.as_str());
        }
        DecodedValue::String(value) => {
            push_named_value_lines(flattened, label, value.as_str());
        }
        DecodedValue::Option(value) => match value {
            Some(value) => collect_decoded_field_lines(flattened, label, value, signer_pubkey),
            None => push_named_value_lines(flattened, label, "none"),
        },
        DecodedValue::Array(values) => {
            if values.is_empty() {
                push_named_value_lines(flattened, label, "[]");
                return;
            }

            for (index, value) in values.iter().enumerate() {
                let nested = format!("{}[{}]", label, index + 1);
                collect_decoded_field_lines(flattened, nested.as_str(), value, signer_pubkey);
            }
        }
        DecodedValue::Struct(fields) => {
            if fields.is_empty() {
                push_named_value_lines(flattened, label, "{}");
                return;
            }

            for field in fields {
                let nested = format!("{}.{}", label, field.name);
                collect_decoded_field_lines(
                    flattened,
                    nested.as_str(),
                    &field.value,
                    signer_pubkey,
                );
            }
        }
        DecodedValue::Enum(variant) => {
            push_named_value_lines(flattened, label, variant.name.as_str());

            if let Some(fields) = &variant.value {
                for DecodedField { name, value } in fields {
                    let nested = format!("{}.{}", label, name);
                    collect_decoded_field_lines(flattened, nested.as_str(), value, signer_pubkey);
                }
            }
        }
    }
}

fn push_header_lines(lines: &mut Vec<ReviewLine>, value: &str) {
    push_wrapped_lines(lines, value, ReviewLineStyle::Header);
}

fn push_instruction_header_lines(
    lines: &mut Vec<ReviewLine>,
    counter: &str,
    program: &str,
    instruction: &str,
) {
    let combined = format!("{counter} {program}");
    let mut header_lines = if text_fits(combined.as_str(), ReviewLineStyle::Header) {
        wrap_text_for_style(combined.as_str(), ReviewLineStyle::Header)
    } else {
        let mut split = Vec::new();
        split.extend(wrap_text_for_style(counter, ReviewLineStyle::Header));
        split.extend(wrap_text_for_style(program, ReviewLineStyle::Header));
        split
    };
    header_lines.extend(wrap_text_for_style(instruction, ReviewLineStyle::Header));

    for text in header_lines {
        lines.push(ReviewLine {
            text,
            style: ReviewLineStyle::Header,
        });
    }
}

fn push_named_value_lines(lines: &mut Vec<ReviewLine>, label: &str, value: &str) {
    push_wrapped_lines(lines, label, ReviewLineStyle::Label);
    push_wrapped_lines(lines, value, ReviewLineStyle::Value);
}

fn push_inline_value_lines(lines: &mut Vec<ReviewLine>, value: &str) {
    push_wrapped_lines(lines, value, ReviewLineStyle::Label);
}

fn push_wrapped_lines(lines: &mut Vec<ReviewLine>, value: &str, style: ReviewLineStyle) {
    for text in wrap_text_for_style(value, style) {
        lines.push(ReviewLine { text, style });
    }
}

fn wrap_text_for_style(value: &str, style: ReviewLineStyle) -> Vec<String> {
    let width = match style {
        ReviewLineStyle::Value => value_text_width(),
        ReviewLineStyle::Header | ReviewLineStyle::Label => text_width(),
    };
    wrap_text_with_width(value, width, matches!(style, ReviewLineStyle::Header))
}

fn text_fits(value: &str, style: ReviewLineStyle) -> bool {
    let width = match style {
        ReviewLineStyle::Value => value_text_width(),
        ReviewLineStyle::Header | ReviewLineStyle::Label => text_width(),
    };
    text_pixel_width(value, matches!(style, ReviewLineStyle::Header)) <= width
}

fn wrap_text_with_width(value: &str, max_width: usize, bold: bool) -> Vec<String> {
    if value.is_empty() {
        let mut lines = Vec::new();
        lines.push(String::new());
        return lines;
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for byte in value.bytes() {
        let glyph = normalize_glyph_byte(byte);
        let glyph_width = glyph_width(glyph, bold);

        if !current.is_empty() && current_width + glyph_width > max_width {
            lines.push(current);
            current = String::new();
            current_width = 0;
        }

        current.push(glyph as char);
        current_width += glyph_width;
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

fn text_pixel_width(value: &str, bold: bool) -> usize {
    value
        .bytes()
        .map(normalize_glyph_byte)
        .map(|glyph| glyph_width(glyph, bold))
        .sum()
}

fn render_number(number: &DecodedNumber) -> String {
    match number {
        DecodedNumber::U8(value) => format_grouped_u64(*value as u64),
        DecodedNumber::U16(value) => format_grouped_u64(*value as u64),
        DecodedNumber::U32(value) => format_grouped_u64(*value as u64),
        DecodedNumber::U64(value) => format_grouped_u64(*value),
        DecodedNumber::I64(value) => {
            if *value < 0 {
                format!("-{}", format_grouped_u64(value.unsigned_abs()))
            } else {
                format_grouped_u64(*value as u64)
            }
        }
    }
}

fn format_grouped_u64(value: u64) -> String {
    let digits = format!("{value}");
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let bytes = digits.as_bytes();

    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 && (bytes.len() - index) % 3 == 0 {
            out.push('_');
        }
        out.push(*byte as char);
    }

    out
}

fn format_account_ref(account: AccountRefView<'_>, signer_pubkey: &[u8; 32]) -> String {
    match account {
        AccountRefView::Static(static_account) => {
            format_static_account(static_account, signer_pubkey)
        }
        AccountRefView::Lookup(lookup_account) => format_lookup_account(lookup_account),
    }
}

fn format_static_account(account: StaticAccountRefView<'_>, signer_pubkey: &[u8; 32]) -> String {
    if account.pubkey == signer_pubkey {
        String::from("<wallet>")
    } else {
        format_base58(account.pubkey)
    }
}

fn format_lookup_account(account: LookupAccountRefView<'_>) -> String {
    let access = if account.writable { "w" } else { "r" };
    let table = format_base58(account.table_account);
    format!("ALT {}[{}] {}", access, account.table_index, table)
}

fn format_base58(bytes: &[u8]) -> String {
    bs58::encode(bytes).into_string()
}

fn format_message_hash(message: &[u8]) -> Result<String, AppSW> {
    let mut digest = [0u8; MESSAGE_HASH_LENGTH];
    Sha2_256::new()
        .hash(message, &mut digest)
        .map_err(|_| AppSW::CommError)?;
    Ok(format_base58(&digest))
}

fn show_review(sections: &[ReviewSection]) -> Result<bool, AppSW> {
    let mut buttons = ButtonsState::new();
    let mut section_index = 0usize;
    let mut scroll_line = 0usize;

    loop {
        let section = &sections[section_index];
        let visible_lines = visible_line_count();
        draw_section(
            section,
            section_index,
            sections.len(),
            scroll_line,
            visible_lines,
        );

        match get_event(&mut buttons) {
            Some(ledger_secure_sdk_sys::buttons::ButtonEvent::LeftButtonRelease) => {
                if scroll_line > 0 {
                    scroll_line -= 1;
                }
            }
            Some(ledger_secure_sdk_sys::buttons::ButtonEvent::RightButtonRelease) => {
                if scroll_line + visible_lines < section.lines.len() {
                    scroll_line += 1;
                }
            }
            Some(ledger_secure_sdk_sys::buttons::ButtonEvent::BothButtonsRelease) => {
                if section_index + 1 < sections.len() {
                    section_index += 1;
                    scroll_line = 0;
                } else {
                    return Ok(Validator::new("Sign").ask());
                }
            }
            _ => {}
        }
    }
}

fn visible_line_count() -> usize {
    core::cmp::max(
        1,
        (SCREEN_HEIGHT - TOP_PADDING - FOOTER_HEIGHT) / LINE_HEIGHT,
    )
}

fn draw_section(
    section: &ReviewSection,
    section_index: usize,
    section_count: usize,
    scroll_line: usize,
    visible_lines: usize,
) {
    clear_screen();

    for (index, line) in section
        .lines
        .iter()
        .skip(scroll_line)
        .take(visible_lines)
        .enumerate()
    {
        let y = TOP_PADDING + index * LINE_HEIGHT;
        if line.style == ReviewLineStyle::Header {
            RectFull::new()
                .pos(0, y as i32)
                .width(SCREEN_WIDTH as u32)
                .height(LINE_HEIGHT as u32)
                .display();
        }
        draw_text_line(line, y);
    }

    draw_footer(
        section_index,
        section_count,
        scroll_line > 0,
        scroll_line + visible_lines < section.lines.len(),
    );

    screen_update();
}

fn draw_footer(
    section_index: usize,
    section_count: usize,
    can_scroll_up: bool,
    can_scroll_down: bool,
) {
    if can_scroll_up {
        draw_footer_icon(UP_S_ARROW, HORIZONTAL_PADDING as i16);
    }

    let center_label = if section_index + 1 < section_count {
        "next"
    } else {
        "sign"
    };
    let center_width = text_pixel_width(center_label, false) as i32;
    let center_x = ((SCREEN_WIDTH as i32 - center_width) / 2).max(HORIZONTAL_PADDING as i32);
    draw_text_at(center_x, FOOTER_TEXT_Y, center_label, false, false);

    if can_scroll_down {
        let right_x =
            SCREEN_WIDTH as i16 - HORIZONTAL_PADDING as i16 - DOWN_S_ARROW.icon.width as i16;
        draw_footer_icon(DOWN_S_ARROW, right_x);
    }
}

fn draw_footer_icon(icon: Icon<'static>, x: i16) {
    icon.set_x(x).set_y(FOOTER_TEXT_Y as i16).display();
}

fn draw_text_line(line: &ReviewLine, y: usize) {
    let (x, bold, inverted) = match line.style {
        ReviewLineStyle::Header => (HORIZONTAL_PADDING, true, true),
        ReviewLineStyle::Label => (HORIZONTAL_PADDING, false, false),
        ReviewLineStyle::Value => (HORIZONTAL_PADDING + VALUE_INDENT_PIXELS, false, false),
    };

    draw_text_at(x as i32, y, line.text.as_str(), bold, inverted);
}

fn draw_text_at(x: i32, y: usize, text: &str, bold: bool, inverted: bool) {
    let font_choice = bold as usize;
    let mut cur_x = x;

    for byte in text.bytes() {
        let glyph = normalize_glyph_byte(byte);
        let offset = (glyph - 0x20) as usize;
        let bitmap = unsafe {
            let tmp =
                pic(OPEN_SANS[font_choice].chars.0[offset].as_ptr() as *mut c_void) as *const u8;
            core::slice::from_raw_parts(tmp, OPEN_SANS[font_choice].chars.0[offset].len())
        };
        let width = OPEN_SANS[font_choice].dims[offset];
        draw(
            cur_x,
            y as i32,
            width as u32,
            OPEN_SANS[font_choice].height as u32,
            inverted,
            bitmap,
        );
        cur_x += width as i32;
    }
}

fn text_width() -> usize {
    SCREEN_WIDTH - (HORIZONTAL_PADDING * 2)
}

fn value_text_width() -> usize {
    SCREEN_WIDTH - (HORIZONTAL_PADDING * 2) - VALUE_INDENT_PIXELS
}

fn glyph_width(glyph: u8, bold: bool) -> usize {
    OPEN_SANS[bold as usize].dims[(glyph - 0x20) as usize] as usize
}

fn normalize_glyph_byte(byte: u8) -> u8 {
    if (0x20..=0x7f).contains(&byte) {
        byte
    } else {
        b'?'
    }
}

extern "C" {
    fn pic(link_address: *mut c_void) -> *mut c_void;
}
