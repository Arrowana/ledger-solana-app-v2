#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

pub const PUBKEY_LENGTH: usize = 32;
pub const BLOCKHASH_LENGTH: usize = 32;

pub type PubkeyBytes = [u8; PUBKEY_LENGTH];
pub type BlockhashBytes = [u8; BLOCKHASH_LENGTH];

pub type Result<T> = core::result::Result<T, ParseError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageVersion {
    Legacy,
    V0,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    pub num_required_signatures: u8,
    pub num_readonly_signed_accounts: u8,
    pub num_readonly_unsigned_accounts: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledInstruction {
    pub program_id_index: u8,
    pub account_indexes: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressTableLookup {
    pub account_key: PubkeyBytes,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage {
    pub version: MessageVersion,
    pub header: MessageHeader,
    pub static_accounts: Vec<PubkeyBytes>,
    pub recent_blockhash: BlockhashBytes,
    pub instructions: Vec<CompiledInstruction>,
    pub address_table_lookups: Vec<AddressTableLookup>,
}

#[derive(Debug, Clone, Copy)]
pub struct MessageView<'a> {
    input: &'a [u8],
    pub version: MessageVersion,
    pub header: MessageHeader,
    static_accounts_offset: usize,
    static_account_count: usize,
    recent_blockhash_offset: usize,
    instructions_offset: usize,
    instruction_count: usize,
    address_table_lookups_offset: usize,
    address_table_lookup_count: usize,
    loaded_writable_count: usize,
    loaded_readonly_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledInstructionView<'a> {
    pub index: usize,
    pub program_id_index: u8,
    pub account_indexes: &'a [u8],
    pub data: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressTableLookupView<'a> {
    pub account_key: &'a [u8; PUBKEY_LENGTH],
    pub writable_indexes: &'a [u8],
    pub readonly_indexes: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRefView<'a> {
    Static(StaticAccountRefView<'a>),
    Lookup(LookupAccountRefView<'a>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticAccountRefView<'a> {
    pub global_index: u8,
    pub pubkey: &'a [u8; PUBKEY_LENGTH],
    pub signer: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookupAccountRefView<'a> {
    pub global_index: u8,
    pub table_account: &'a [u8; PUBKEY_LENGTH],
    pub table_index: u8,
    pub writable: bool,
}

pub struct InstructionIter<'a> {
    cursor: Cursor<'a>,
    remaining: usize,
    index: usize,
}

pub struct AddressTableLookupIter<'a> {
    cursor: Cursor<'a>,
    remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountRef {
    Static(StaticAccountRef),
    Lookup(LookupAccountRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticAccountRef {
    pub global_index: u8,
    pub pubkey: PubkeyBytes,
    pub signer: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupAccountRef {
    pub global_index: u8,
    pub table_account: PubkeyBytes,
    pub table_index: u8,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionReview {
    pub index: usize,
    pub program: AccountRef,
    pub accounts: Vec<AccountRef>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnexpectedEof {
        context: &'static str,
        needed: usize,
        remaining: usize,
    },
    InvalidShortVec {
        context: &'static str,
    },
    UnsupportedVersion(u8),
    InvalidHeader,
    InvalidAccountIndex {
        context: &'static str,
        index: u8,
        account_count: usize,
    },
    TrailingBytes {
        remaining: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof {
                context,
                needed,
                remaining,
            } => write!(
                f,
                "unexpected eof while reading {context}: needed {needed} bytes, had {remaining}"
            ),
            Self::InvalidShortVec { context } => {
                write!(f, "invalid shortvec while reading {context}")
            }
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported Solana message version: {version}")
            }
            Self::InvalidHeader => write!(f, "invalid Solana message header"),
            Self::InvalidAccountIndex {
                context,
                index,
                account_count,
            } => write!(
                f,
                "invalid account index {index} in {context}; message has {account_count} accounts"
            ),
            Self::TrailingBytes { remaining } => {
                write!(f, "message has {remaining} trailing bytes")
            }
        }
    }
}

impl<'a> MessageView<'a> {
    pub fn try_new(input: &'a [u8]) -> Result<Self> {
        let mut cursor = Cursor::new(input);
        let version_marker = cursor.peek_u8("message prefix")?;
        let version = if version_marker & 0x80 != 0 {
            let version = cursor.read_u8("message version")? & 0x7f;
            match version {
                0 => MessageVersion::V0,
                other => return Err(ParseError::UnsupportedVersion(other)),
            }
        } else {
            MessageVersion::Legacy
        };

        let header = MessageHeader {
            num_required_signatures: cursor.read_u8("num_required_signatures")?,
            num_readonly_signed_accounts: cursor.read_u8("num_readonly_signed_accounts")?,
            num_readonly_unsigned_accounts: cursor.read_u8("num_readonly_unsigned_accounts")?,
        };

        let static_account_count = cursor.read_shortvec("static account count")?;
        let static_accounts_offset = cursor.offset;
        cursor.skip(static_account_count * PUBKEY_LENGTH, "static account keys")?;
        validate_header(header, static_account_count)?;

        let recent_blockhash_offset = cursor.offset;
        cursor.skip(BLOCKHASH_LENGTH, "recent blockhash")?;

        let instruction_count = cursor.read_shortvec("instruction count")?;
        let instructions_offset = cursor.offset;
        for _ in 0..instruction_count {
            skip_compiled_instruction(&mut cursor)?;
        }

        let address_table_lookups_offset = cursor.offset;
        let (address_table_lookup_count, loaded_writable_count, loaded_readonly_count) =
            if matches!(version, MessageVersion::V0) {
                let count = cursor.read_shortvec("address table lookup count")?;
                let mut loaded_writable_count = 0usize;
                let mut loaded_readonly_count = 0usize;
                for _ in 0..count {
                    cursor.skip(PUBKEY_LENGTH, "address table account key")?;
                    let writable_count = cursor.read_shortvec("address table writable count")?;
                    loaded_writable_count += writable_count;
                    cursor.skip(writable_count, "address table writable indexes")?;
                    let readonly_count = cursor.read_shortvec("address table readonly count")?;
                    loaded_readonly_count += readonly_count;
                    cursor.skip(readonly_count, "address table readonly indexes")?;
                }
                (count, loaded_writable_count, loaded_readonly_count)
            } else {
                (0, 0, 0)
            };

        if cursor.remaining() != 0 {
            return Err(ParseError::TrailingBytes {
                remaining: cursor.remaining(),
            });
        }

        let view = Self {
            input,
            version,
            header,
            static_accounts_offset,
            static_account_count,
            recent_blockhash_offset,
            instructions_offset,
            instruction_count,
            address_table_lookups_offset,
            address_table_lookup_count,
            loaded_writable_count,
            loaded_readonly_count,
        };

        validate_instruction_indexes_in_view(&view)?;
        Ok(view)
    }

    pub fn static_account_count(&self) -> usize {
        self.static_account_count
    }

    pub fn instruction_count(&self) -> usize {
        self.instruction_count
    }

    pub fn address_table_lookup_count(&self) -> usize {
        self.address_table_lookup_count
    }

    pub fn total_account_count(&self) -> usize {
        self.static_account_count + self.loaded_writable_count + self.loaded_readonly_count
    }

    pub fn static_account(&self, index: u8) -> Option<&'a [u8; PUBKEY_LENGTH]> {
        let index = index as usize;
        if index >= self.static_account_count {
            return None;
        }
        let start = self.static_accounts_offset + index * PUBKEY_LENGTH;
        let end = start + PUBKEY_LENGTH;
        as_pubkey_ref(&self.input[start..end])
    }

    pub fn recent_blockhash(&self) -> &'a [u8; BLOCKHASH_LENGTH] {
        as_blockhash_ref(
            &self.input
                [self.recent_blockhash_offset..self.recent_blockhash_offset + BLOCKHASH_LENGTH],
        )
        .unwrap()
    }

    pub fn instructions(&self) -> InstructionIter<'a> {
        InstructionIter {
            cursor: Cursor::new_at(self.input, self.instructions_offset),
            remaining: self.instruction_count,
            index: 0,
        }
    }

    pub fn address_table_lookups(&self) -> AddressTableLookupIter<'a> {
        let cursor = if matches!(self.version, MessageVersion::V0) {
            Cursor::new_at(
                self.input,
                self.address_table_lookups_offset + shortvec_len(self.address_table_lookup_count),
            )
        } else {
            Cursor::new_at(self.input, self.address_table_lookups_offset)
        };
        AddressTableLookupIter {
            cursor,
            remaining: self.address_table_lookup_count,
        }
    }

    pub fn instruction_review(&self, instruction_index: usize) -> Result<InstructionReview> {
        let instruction = self.instructions().nth(instruction_index).ok_or(
            ParseError::InvalidAccountIndex {
                context: "instruction review",
                index: instruction_index as u8,
                account_count: self.instruction_count,
            },
        )??;
        let program = account_ref_view_to_owned(self.account_ref(instruction.program_id_index)?);
        let mut accounts = Vec::with_capacity(instruction.account_indexes.len());
        for account_index in instruction.account_indexes {
            accounts.push(account_ref_view_to_owned(self.account_ref(*account_index)?));
        }

        Ok(InstructionReview {
            index: instruction.index,
            program,
            accounts,
            data: instruction.data.to_vec(),
        })
    }

    pub fn account_ref(&self, global_index: u8) -> Result<AccountRefView<'a>> {
        let global_index_usize = global_index as usize;
        if global_index_usize < self.static_account_count {
            let signer_count = self.header.num_required_signatures as usize;
            let writable_signer_count =
                signer_count.saturating_sub(self.header.num_readonly_signed_accounts as usize);
            let writable_unsigned_end = self
                .static_account_count
                .saturating_sub(self.header.num_readonly_unsigned_accounts as usize);
            let signer = global_index_usize < signer_count;
            let writable = if signer {
                global_index_usize < writable_signer_count
            } else {
                global_index_usize < writable_unsigned_end
            };
            return Ok(AccountRefView::Static(StaticAccountRefView {
                global_index,
                pubkey: self.static_account(global_index).ok_or(
                    ParseError::InvalidAccountIndex {
                        context: "static account",
                        index: global_index,
                        account_count: self.total_account_count(),
                    },
                )?,
                signer,
                writable,
            }));
        }

        let mut relative_index = global_index_usize
            .checked_sub(self.static_account_count)
            .ok_or(ParseError::InvalidAccountIndex {
                context: "lookup account",
                index: global_index,
                account_count: self.total_account_count(),
            })?;

        for lookup in self.address_table_lookups() {
            let lookup = lookup?;
            if relative_index < lookup.writable_indexes.len() {
                return Ok(AccountRefView::Lookup(LookupAccountRefView {
                    global_index,
                    table_account: lookup.account_key,
                    table_index: lookup.writable_indexes[relative_index],
                    writable: true,
                }));
            }
            relative_index -= lookup.writable_indexes.len();
        }

        for lookup in self.address_table_lookups() {
            let lookup = lookup?;
            if relative_index < lookup.readonly_indexes.len() {
                return Ok(AccountRefView::Lookup(LookupAccountRefView {
                    global_index,
                    table_account: lookup.account_key,
                    table_index: lookup.readonly_indexes[relative_index],
                    writable: false,
                }));
            }
            relative_index -= lookup.readonly_indexes.len();
        }

        Err(ParseError::InvalidAccountIndex {
            context: "lookup account",
            index: global_index,
            account_count: self.total_account_count(),
        })
    }
}

impl ParsedMessage {
    fn from_view(view: &MessageView<'_>) -> Result<Self> {
        let mut static_accounts = Vec::with_capacity(view.static_account_count());
        for index in 0..view.static_account_count() {
            static_accounts.push(*view.static_account(index as u8).unwrap());
        }

        let mut instructions = Vec::with_capacity(view.instruction_count());
        for instruction in view.instructions() {
            let instruction = instruction?;
            instructions.push(CompiledInstruction {
                program_id_index: instruction.program_id_index,
                account_indexes: instruction.account_indexes.to_vec(),
                data: instruction.data.to_vec(),
            });
        }

        let mut address_table_lookups = Vec::with_capacity(view.address_table_lookup_count());
        for lookup in view.address_table_lookups() {
            let lookup = lookup?;
            address_table_lookups.push(AddressTableLookup {
                account_key: *lookup.account_key,
                writable_indexes: lookup.writable_indexes.to_vec(),
                readonly_indexes: lookup.readonly_indexes.to_vec(),
            });
        }

        Ok(Self {
            version: view.version,
            header: view.header,
            static_accounts,
            recent_blockhash: *view.recent_blockhash(),
            instructions,
            address_table_lookups,
        })
    }
}

impl ParsedMessage {
    pub fn total_account_count(&self) -> usize {
        self.static_accounts.len()
            + self
                .address_table_lookups
                .iter()
                .map(|lookup| lookup.writable_indexes.len() + lookup.readonly_indexes.len())
                .sum::<usize>()
    }

    pub fn account_ref(&self, global_index: u8) -> Option<AccountRef> {
        let global_index_usize = global_index as usize;
        if global_index_usize < self.static_accounts.len() {
            let signer_count = self.header.num_required_signatures as usize;
            let writable_signer_count =
                signer_count.saturating_sub(self.header.num_readonly_signed_accounts as usize);
            let writable_unsigned_end = self
                .static_accounts
                .len()
                .saturating_sub(self.header.num_readonly_unsigned_accounts as usize);
            let signer = global_index_usize < signer_count;
            let writable = if signer {
                global_index_usize < writable_signer_count
            } else {
                global_index_usize < writable_unsigned_end
            };
            return Some(AccountRef::Static(StaticAccountRef {
                global_index,
                pubkey: self.static_accounts[global_index_usize],
                signer,
                writable,
            }));
        }

        let mut relative_index = global_index_usize.checked_sub(self.static_accounts.len())?;
        for lookup in &self.address_table_lookups {
            if relative_index < lookup.writable_indexes.len() {
                return Some(AccountRef::Lookup(LookupAccountRef {
                    global_index,
                    table_account: lookup.account_key,
                    table_index: lookup.writable_indexes[relative_index],
                    writable: true,
                }));
            }
            relative_index -= lookup.writable_indexes.len();
        }

        for lookup in &self.address_table_lookups {
            if relative_index < lookup.readonly_indexes.len() {
                return Some(AccountRef::Lookup(LookupAccountRef {
                    global_index,
                    table_account: lookup.account_key,
                    table_index: lookup.readonly_indexes[relative_index],
                    writable: false,
                }));
            }
            relative_index -= lookup.readonly_indexes.len();
        }

        None
    }

    pub fn instruction_review(&self, instruction_index: usize) -> Option<InstructionReview> {
        let instruction = self.instructions.get(instruction_index)?;
        let program = self.account_ref(instruction.program_id_index)?;
        let mut accounts = Vec::with_capacity(instruction.account_indexes.len());
        for account_index in &instruction.account_indexes {
            accounts.push(self.account_ref(*account_index)?);
        }

        Some(InstructionReview {
            index: instruction_index,
            program,
            accounts,
            data: instruction.data.clone(),
        })
    }
}

impl<'a> Iterator for InstructionIter<'a> {
    type Item = Result<CompiledInstructionView<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        self.remaining -= 1;
        let program_id_index = match self.cursor.read_u8("instruction program_id_index") {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        let account_count = match self.cursor.read_shortvec("instruction account count") {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        let account_indexes = match self
            .cursor
            .read_bytes(account_count, "instruction account indexes")
        {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        let data_len = match self.cursor.read_shortvec("instruction data length") {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        let data = match self.cursor.read_bytes(data_len, "instruction data") {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };

        let view = CompiledInstructionView {
            index: self.index,
            program_id_index,
            account_indexes,
            data,
        };
        self.index += 1;
        Some(Ok(view))
    }
}

impl<'a> Iterator for AddressTableLookupIter<'a> {
    type Item = Result<AddressTableLookupView<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        self.remaining -= 1;
        let account_key = match self
            .cursor
            .read_bytes(PUBKEY_LENGTH, "address table account key")
        {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        let writable_count = match self.cursor.read_shortvec("address table writable count") {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        let writable_indexes = match self
            .cursor
            .read_bytes(writable_count, "address table writable indexes")
        {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        let readonly_count = match self.cursor.read_shortvec("address table readonly count") {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        let readonly_indexes = match self
            .cursor
            .read_bytes(readonly_count, "address table readonly indexes")
        {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };

        Some(Ok(AddressTableLookupView {
            account_key: as_pubkey_ref(account_key).unwrap(),
            writable_indexes,
            readonly_indexes,
        }))
    }
}

pub fn parse_message(input: &[u8]) -> Result<ParsedMessage> {
    let view = MessageView::try_new(input)?;
    ParsedMessage::from_view(&view)
}

fn validate_header(header: MessageHeader, static_account_count: usize) -> Result<()> {
    let signer_count = header.num_required_signatures as usize;
    if signer_count > static_account_count {
        return Err(ParseError::InvalidHeader);
    }
    if header.num_readonly_signed_accounts as usize > signer_count {
        return Err(ParseError::InvalidHeader);
    }
    let unsigned_count = static_account_count.saturating_sub(signer_count);
    if header.num_readonly_unsigned_accounts as usize > unsigned_count {
        return Err(ParseError::InvalidHeader);
    }
    Ok(())
}

fn validate_instruction_indexes_in_view(view: &MessageView<'_>) -> Result<()> {
    let account_count = view.total_account_count();
    for instruction in view.instructions() {
        let instruction = instruction?;
        if instruction.program_id_index as usize >= account_count {
            return Err(ParseError::InvalidAccountIndex {
                context: "instruction program id",
                index: instruction.program_id_index,
                account_count,
            });
        }
        for account_index in instruction.account_indexes {
            if *account_index as usize >= account_count {
                return Err(ParseError::InvalidAccountIndex {
                    context: "instruction account",
                    index: *account_index,
                    account_count,
                });
            }
        }
    }
    Ok(())
}

fn skip_compiled_instruction(cursor: &mut Cursor<'_>) -> Result<()> {
    cursor.skip(1, "instruction program_id_index")?;
    let account_count = cursor.read_shortvec("instruction account count")?;
    cursor.skip(account_count, "instruction account indexes")?;
    let data_len = cursor.read_shortvec("instruction data length")?;
    cursor.skip(data_len, "instruction data")?;
    Ok(())
}

fn account_ref_view_to_owned(account: AccountRefView<'_>) -> AccountRef {
    match account {
        AccountRefView::Static(account) => AccountRef::Static(StaticAccountRef {
            global_index: account.global_index,
            pubkey: *account.pubkey,
            signer: account.signer,
            writable: account.writable,
        }),
        AccountRefView::Lookup(account) => AccountRef::Lookup(LookupAccountRef {
            global_index: account.global_index,
            table_account: *account.table_account,
            table_index: account.table_index,
            writable: account.writable,
        }),
    }
}

fn as_pubkey_ref(bytes: &[u8]) -> Option<&[u8; PUBKEY_LENGTH]> {
    bytes.try_into().ok()
}

fn as_blockhash_ref(bytes: &[u8]) -> Option<&[u8; BLOCKHASH_LENGTH]> {
    bytes.try_into().ok()
}

fn shortvec_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    fn new_at(input: &'a [u8], offset: usize) -> Self {
        Self { input, offset }
    }

    fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.offset)
    }

    fn peek_u8(&self, context: &'static str) -> Result<u8> {
        self.input
            .get(self.offset)
            .copied()
            .ok_or(ParseError::UnexpectedEof {
                context,
                needed: 1,
                remaining: 0,
            })
    }

    fn read_u8(&mut self, context: &'static str) -> Result<u8> {
        Ok(self.read_bytes(1, context)?[0])
    }

    fn read_bytes(&mut self, len: usize, context: &'static str) -> Result<&'a [u8]> {
        let remaining = self.remaining();
        if remaining < len {
            return Err(ParseError::UnexpectedEof {
                context,
                needed: len,
                remaining,
            });
        }

        let start = self.offset;
        self.offset += len;
        Ok(&self.input[start..self.offset])
    }

    fn skip(&mut self, len: usize, context: &'static str) -> Result<()> {
        self.read_bytes(len, context).map(|_| ())
    }

    fn read_shortvec(&mut self, context: &'static str) -> Result<usize> {
        let mut value = 0usize;
        let mut shift = 0usize;
        loop {
            let byte = self.read_u8(context)?;
            let chunk = (byte & 0x7f) as usize;
            if shift >= usize::BITS as usize || (chunk << shift) >> shift != chunk {
                return Err(ParseError::InvalidShortVec { context });
            }
            value |= chunk << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= usize::BITS as usize {
                return Err(ParseError::InvalidShortVec { context });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use codama_parser::{parse_program_index, DecodedNumber, DecodedValue};
    use solana_message::{
        compiled_instruction::CompiledInstruction as UpstreamCompiledInstruction, legacy, v0,
        Address as UpstreamAddress, Hash as UpstreamHash, MessageHeader as UpstreamMessageHeader,
        VersionedMessage as UpstreamVersionedMessage,
    };

    const SAMPLE_IDL: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testdata/sample-program.codama.json"
    ));

    #[test]
    fn iterates_legacy_message_view_and_decodes_review_request_instruction() {
        let message = build_legacy_message(
            &[1u8; 32],
            &[2u8; 32],
            &[3u8; 32],
            &[4u8; 32],
            &review_request_data(42, true),
        );

        let view = MessageView::try_new(&message).unwrap();
        assert_eq!(view.version, MessageVersion::Legacy);
        assert_eq!(view.instruction_count(), 1);

        let instruction = view.instructions().next().unwrap().unwrap();
        let program_ref = view.account_ref(instruction.program_id_index).unwrap();
        assert!(matches!(
            program_ref,
            AccountRefView::Static(StaticAccountRefView {
                pubkey,
                signer: false,
                writable: false,
                ..
            }) if *pubkey == [4u8; 32]
        ));

        let program = parse_program_index(SAMPLE_IDL).unwrap();
        let decoded = program.decode_instruction_data(instruction.data).unwrap();
        assert_eq!(decoded.name, "reviewRequest");
        assert_eq!(decoded.arguments.len(), 2);
        assert!(matches!(
            decoded.arguments[0].value,
            DecodedValue::Number(DecodedNumber::U64(42))
        ));
        assert!(matches!(
            decoded.arguments[1].value,
            DecodedValue::Boolean(true)
        ));
    }

    #[test]
    fn iterates_v0_message_view_with_unresolved_lookup_accounts() {
        let message = build_v0_message();
        let view = MessageView::try_new(&message).unwrap();

        assert_eq!(view.version, MessageVersion::V0);
        assert_eq!(view.total_account_count(), 4);
        assert_eq!(view.address_table_lookup_count(), 1);

        let instruction = view.instructions().next().unwrap().unwrap();
        let second_account = view.account_ref(instruction.account_indexes[1]).unwrap();
        assert!(matches!(
            second_account,
            AccountRefView::Lookup(LookupAccountRefView {
                table_account,
                table_index: 7,
                writable: true,
                ..
            }) if *table_account == [9u8; 32]
        ));
    }

    fn review_request_data(request_index: u64, urgent: bool) -> Vec<u8> {
        let mut out = hex::decode("2122232425262728").unwrap();
        out.extend_from_slice(&request_index.to_le_bytes());
        out.push(u8::from(urgent));
        out
    }

    fn build_legacy_message(
        signer: &[u8; 32],
        request: &[u8; 32],
        resource: &[u8; 32],
        program: &[u8; 32],
        instruction_data: &[u8],
    ) -> Vec<u8> {
        legacy::Message {
            header: UpstreamMessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 2,
            },
            account_keys: vec![
                UpstreamAddress::from(*signer),
                UpstreamAddress::from(*request),
                UpstreamAddress::from(*resource),
                UpstreamAddress::from(*program),
            ],
            recent_blockhash: UpstreamHash::new_from_array([5u8; 32]),
            instructions: vec![UpstreamCompiledInstruction::new_from_raw_parts(
                3,
                instruction_data.to_vec(),
                vec![1, 0, 2],
            )],
        }
        .serialize()
    }

    fn build_v0_message() -> Vec<u8> {
        UpstreamVersionedMessage::V0(v0::Message {
            header: UpstreamMessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![
                UpstreamAddress::from([1u8; 32]),
                UpstreamAddress::from([2u8; 32]),
            ],
            recent_blockhash: UpstreamHash::new_from_array([3u8; 32]),
            instructions: vec![UpstreamCompiledInstruction::new_from_raw_parts(
                1,
                vec![0xaa],
                vec![0, 2],
            )],
            address_table_lookups: vec![v0::MessageAddressTableLookup {
                account_key: UpstreamAddress::from([9u8; 32]),
                writable_indexes: vec![7],
                readonly_indexes: vec![9],
            }],
        })
        .serialize()
    }
}
