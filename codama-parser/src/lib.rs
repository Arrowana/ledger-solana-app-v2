#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use serde::Deserialize;
use serde_json::value::RawValue;

pub type Result<T> = core::result::Result<T, ParseError>;
pub type DecodeResult<T> = core::result::Result<T, DecodeError>;
pub type IndexedDecodeResult<T> = core::result::Result<T, IndexedDecodeError>;

#[derive(Debug)]
pub enum ParseError {
    Json(serde_json::Error),
    InvalidRootKind(String),
    InvalidStandard(String),
    MissingDiscriminator {
        instruction: String,
    },
    InvalidDiscriminatorEncoding {
        instruction: String,
        encoding: String,
    },
    InvalidDiscriminatorHex {
        instruction: String,
    },
    MissingDefinedType {
        context: String,
        name: String,
    },
    UnsupportedTypeNode {
        context: String,
        kind: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(err) => write!(f, "json parse error: {err}"),
            Self::InvalidRootKind(kind) => write!(f, "unsupported root kind: {kind}"),
            Self::InvalidStandard(standard) => write!(f, "unsupported codama standard: {standard}"),
            Self::MissingDiscriminator { instruction } => {
                write!(
                    f,
                    "missing discriminator argument for instruction: {instruction}"
                )
            }
            Self::InvalidDiscriminatorEncoding {
                instruction,
                encoding,
            } => write!(
                f,
                "unsupported discriminator encoding for instruction {instruction}: {encoding}"
            ),
            Self::InvalidDiscriminatorHex { instruction } => {
                write!(
                    f,
                    "invalid discriminator hex for instruction: {instruction}"
                )
            }
            Self::MissingDefinedType { context, name } => {
                write!(f, "missing defined type {name} referenced from {context}")
            }
            Self::UnsupportedTypeNode { context, kind } => {
                write!(f, "unsupported type node in {context}: {kind}")
            }
        }
    }
}

impl From<serde_json::Error> for ParseError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug)]
pub enum IndexedDecodeError {
    Parse(ParseError),
    Decode(DecodeError),
}

impl fmt::Display for IndexedDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "{err}"),
            Self::Decode(err) => write!(f, "{err}"),
        }
    }
}

impl From<ParseError> for IndexedDecodeError {
    fn from(value: ParseError) -> Self {
        Self::Parse(value)
    }
}

impl From<DecodeError> for IndexedDecodeError {
    fn from(value: DecodeError) -> Self {
        Self::Decode(value)
    }
}

#[derive(Debug)]
pub struct ProgramIndex<'a> {
    pub name: String,
    pub public_key: String,
    pub version: String,
    pub origin: Option<String>,
    pub instructions: Vec<InstructionSelector<'a>>,
    defined_types: Vec<DefinedTypeEntry<'a>>,
}

impl<'a> ProgramIndex<'a> {
    pub fn instruction_by_name(&self, name: &str) -> Option<&InstructionSelector<'a>> {
        self.instructions.iter().find(|ix| ix.name == name)
    }

    pub fn instruction_by_selector(&self, selector: &[u8]) -> Option<&InstructionSelector<'a>> {
        self.instructions
            .iter()
            .find(|ix| ix.selector.as_slice() == selector)
    }

    pub fn load_instruction_schema_by_name(
        &self,
        name: &str,
    ) -> Result<Option<InstructionSchemaBundle>> {
        self.instruction_by_name(name)
            .map(|instruction| self.load_instruction_schema(instruction))
            .transpose()
    }

    pub fn load_instruction_schema_by_selector(
        &self,
        selector: &[u8],
    ) -> Result<Option<InstructionSchemaBundle>> {
        self.instruction_by_selector(selector)
            .map(|instruction| self.load_instruction_schema(instruction))
            .transpose()
    }

    pub fn decode_instruction_data(&self, data: &[u8]) -> IndexedDecodeResult<DecodedInstruction> {
        let instruction = self
            .instructions
            .iter()
            .find(|ix| data.starts_with(ix.selector.as_slice()))
            .ok_or(DecodeError::UnknownInstructionSelector)?;
        let bundle = self.load_instruction_schema(instruction)?;
        Ok(bundle.decode_instruction_data(data)?)
    }

    fn load_instruction_schema(
        &self,
        instruction: &InstructionSelector<'a>,
    ) -> Result<InstructionSchemaBundle> {
        let raw_instruction: RawInstructionNode = serde_json::from_str(instruction.raw_json.get())?;
        let instruction_schema = InstructionSchema::from_raw(raw_instruction)?;
        let defined_types = self.resolve_defined_types(&instruction_schema)?;
        Ok(InstructionSchemaBundle {
            instruction: instruction_schema,
            defined_types,
        })
    }

    fn resolve_defined_types(
        &self,
        instruction: &InstructionSchema,
    ) -> Result<Vec<DefinedTypeSchema>> {
        let mut pending = Vec::new();
        for argument in &instruction.arguments {
            collect_defined_type_names(&argument.ty, &mut pending);
        }

        let mut resolved: Vec<DefinedTypeSchema> = Vec::new();
        let mut index = 0;
        while index < pending.len() {
            let name = pending[index].clone();
            index += 1;

            if resolved.iter().any(|defined| defined.name == name) {
                continue;
            }

            let raw_defined_type = self
                .defined_types
                .iter()
                .find(|defined| defined.name == name)
                .ok_or(ParseError::MissingDefinedType {
                    context: instruction.name.clone(),
                    name: name.clone(),
                })?;

            let parsed: RawDefinedTypeNode = serde_json::from_str(raw_defined_type.raw_json.get())?;
            let defined_type = DefinedTypeSchema::from_raw(parsed)?;
            collect_defined_type_names(&defined_type.ty, &mut pending);
            resolved.push(defined_type);
        }

        Ok(resolved)
    }
}

#[derive(Debug)]
pub struct InstructionSelector<'a> {
    pub name: String,
    pub selector: Vec<u8>,
    raw_json: &'a RawValue,
}

#[derive(Debug)]
struct DefinedTypeEntry<'a> {
    name: String,
    raw_json: &'a RawValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramSchema {
    pub name: String,
    pub public_key: String,
    pub version: String,
    pub origin: Option<String>,
    pub instructions: Vec<InstructionSchema>,
    pub defined_types: Vec<DefinedTypeSchema>,
}

impl ProgramSchema {
    pub fn instruction_by_name(&self, name: &str) -> Option<&InstructionSchema> {
        self.instructions.iter().find(|ix| ix.name == name)
    }

    pub fn instruction_by_selector(&self, selector: &[u8]) -> Option<&InstructionSchema> {
        self.instructions
            .iter()
            .find(|ix| ix.selector.as_slice() == selector)
    }

    pub fn defined_type(&self, name: &str) -> Option<&DefinedTypeSchema> {
        self.defined_types.iter().find(|ty| ty.name == name)
    }

    pub fn decode_instruction_data(&self, data: &[u8]) -> DecodeResult<DecodedInstruction> {
        let instruction = self
            .instructions
            .iter()
            .find(|ix| data.starts_with(ix.selector.as_slice()))
            .ok_or(DecodeError::UnknownInstructionSelector)?;
        decode_instruction_data_with_types(&self.defined_types, instruction, data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionSchema {
    pub name: String,
    pub selector: Vec<u8>,
    pub arguments: Vec<ArgumentSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgumentSchema {
    pub name: String,
    pub ty: TypeNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedTypeSchema {
    pub name: String,
    pub ty: TypeNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionSchemaBundle {
    pub instruction: InstructionSchema,
    pub defined_types: Vec<DefinedTypeSchema>,
}

impl InstructionSchemaBundle {
    pub fn defined_type(&self, name: &str) -> Option<&DefinedTypeSchema> {
        self.defined_types.iter().find(|ty| ty.name == name)
    }

    pub fn decode_instruction_data(&self, data: &[u8]) -> DecodeResult<DecodedInstruction> {
        decode_instruction_data_with_types(&self.defined_types, &self.instruction, data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedInstruction {
    pub name: String,
    pub selector: Vec<u8>,
    pub arguments: Vec<DecodedField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedField {
    pub name: String,
    pub value: DecodedValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedValue {
    Number(DecodedNumber),
    Boolean(bool),
    PublicKey([u8; 32]),
    Bytes(Vec<u8>),
    String(String),
    Option(Option<Box<DecodedValue>>),
    Array(Vec<DecodedValue>),
    Struct(Vec<DecodedField>),
    Enum(DecodedEnumVariant),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedNumber {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I64(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedEnumVariant {
    pub name: String,
    pub value: Option<Vec<DecodedField>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    UnknownInstructionSelector,
    UnknownDefinedType(String),
    InvalidBoolValue {
        context: String,
        value: DecodedNumber,
    },
    InvalidOptionTag {
        context: String,
        value: u64,
    },
    InvalidEnumDiscriminator {
        context: String,
        value: u64,
    },
    UnsupportedUnboundedType {
        context: String,
        kind: &'static str,
    },
    InvalidUtf8 {
        context: String,
    },
    UnexpectedEof {
        context: String,
        needed: usize,
        remaining: usize,
    },
    TrailingBytes {
        remaining: usize,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownInstructionSelector => write!(f, "unknown instruction selector"),
            Self::UnknownDefinedType(name) => write!(f, "unknown defined type: {name}"),
            Self::InvalidBoolValue { context, value } => {
                write!(f, "invalid bool value in {context}: {value:?}")
            }
            Self::InvalidOptionTag { context, value } => {
                write!(f, "invalid option tag in {context}: {value}")
            }
            Self::InvalidEnumDiscriminator { context, value } => {
                write!(f, "invalid enum discriminator in {context}: {value}")
            }
            Self::UnsupportedUnboundedType { context, kind } => {
                write!(f, "unsupported unbounded {kind} in {context}")
            }
            Self::InvalidUtf8 { context } => write!(f, "invalid utf8 in {context}"),
            Self::UnexpectedEof {
                context,
                needed,
                remaining,
            } => write!(
                f,
                "unexpected eof in {context}: needed {needed} bytes, had {remaining}"
            ),
            Self::TrailingBytes { remaining } => {
                write!(f, "instruction payload has {remaining} trailing bytes")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeNode {
    Number(NumberType),
    Boolean {
        size: NumberType,
    },
    PublicKey,
    Bytes,
    String {
        encoding: StringEncoding,
    },
    FixedSize {
        size: u32,
        item: Box<TypeNode>,
    },
    SizePrefix {
        item: Box<TypeNode>,
        prefix: NumberType,
    },
    Option {
        item: Box<TypeNode>,
        prefix: NumberType,
        fixed: bool,
    },
    Array {
        item: Box<TypeNode>,
        count: CountNode,
    },
    Struct {
        fields: Vec<StructField>,
    },
    Enum {
        variants: Vec<EnumVariant>,
        size: NumberType,
    },
    Defined(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: TypeNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnumVariant {
    Empty {
        name: String,
        discriminator: Option<u64>,
    },
    Struct {
        name: String,
        discriminator: Option<u64>,
        fields: Vec<StructField>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountNode {
    Fixed(u32),
    Prefixed(NumberType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringEncoding {
    Utf8,
    Base16,
    Base58,
    Base64,
}

impl StringEncoding {
    fn parse(value: &str, context: &str) -> Result<Self> {
        match value {
            "utf8" => Ok(Self::Utf8),
            "base16" => Ok(Self::Base16),
            "base58" => Ok(Self::Base58),
            "base64" => Ok(Self::Base64),
            other => Err(ParseError::UnsupportedTypeNode {
                context: context.to_string(),
                kind: format!("string encoding {other}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberEndian {
    Little,
    Big,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberFormat {
    U8,
    U16,
    U32,
    U64,
    I64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberType {
    pub format: NumberFormat,
    pub endian: NumberEndian,
}

impl NumberType {
    fn from_raw(raw: RawNumberTypeNode, context: &str) -> Result<Self> {
        let format = match raw.format.as_str() {
            "u8" => NumberFormat::U8,
            "u16" => NumberFormat::U16,
            "u32" => NumberFormat::U32,
            "u64" => NumberFormat::U64,
            "i64" => NumberFormat::I64,
            other => {
                return Err(ParseError::UnsupportedTypeNode {
                    context: context.to_string(),
                    kind: format!("number format {other}"),
                });
            }
        };
        let endian = match raw.endian.as_deref().unwrap_or("le") {
            "le" => NumberEndian::Little,
            "be" => NumberEndian::Big,
            other => {
                return Err(ParseError::UnsupportedTypeNode {
                    context: context.to_string(),
                    kind: format!("number endian {other}"),
                });
            }
        };
        Ok(Self { format, endian })
    }
}

pub fn parse_program_schema(json: &[u8]) -> Result<ProgramSchema> {
    let raw: RawRootNode = serde_json::from_slice(json)?;
    if raw.kind != "rootNode" {
        return Err(ParseError::InvalidRootKind(raw.kind));
    }
    if raw.standard != "codama" {
        return Err(ParseError::InvalidStandard(raw.standard));
    }

    let instructions = raw
        .program
        .instructions
        .into_iter()
        .map(InstructionSchema::from_raw)
        .collect::<Result<Vec<_>>>()?;

    let defined_types = raw
        .program
        .defined_types
        .into_iter()
        .map(DefinedTypeSchema::from_raw)
        .collect::<Result<Vec<_>>>()?;

    Ok(ProgramSchema {
        name: raw.program.name,
        public_key: raw.program.public_key,
        version: raw.program.version,
        origin: raw.program.origin,
        instructions,
        defined_types,
    })
}

pub fn parse_program_index<'a>(json: &'a [u8]) -> Result<ProgramIndex<'a>> {
    let raw: RawIndexedRootNode<'a> = serde_json::from_slice(json)?;
    if raw.kind != "rootNode" {
        return Err(ParseError::InvalidRootKind(raw.kind.to_string()));
    }
    if raw.standard != "codama" {
        return Err(ParseError::InvalidStandard(raw.standard.to_string()));
    }

    let mut instructions = Vec::with_capacity(raw.program.instructions.len());
    for raw_instruction in raw.program.instructions {
        let parsed: RawInstructionSelectorNode = serde_json::from_str(raw_instruction.get())?;
        instructions.push(InstructionSelector {
            name: parsed.name.clone(),
            selector: parsed.selector_bytes()?,
            raw_json: raw_instruction,
        });
    }

    let mut defined_types = Vec::with_capacity(raw.program.defined_types.len());
    for raw_defined_type in raw.program.defined_types {
        let parsed: RawDefinedTypeNameNode = serde_json::from_str(raw_defined_type.get())?;
        defined_types.push(DefinedTypeEntry {
            name: parsed.name,
            raw_json: raw_defined_type,
        });
    }

    Ok(ProgramIndex {
        name: raw.program.name,
        public_key: raw.program.public_key,
        version: raw.program.version,
        origin: raw.program.origin,
        instructions,
        defined_types,
    })
}

fn decode_instruction_data_with_types(
    defined_types: &[DefinedTypeSchema],
    instruction: &InstructionSchema,
    data: &[u8],
) -> DecodeResult<DecodedInstruction> {
    if !data.starts_with(instruction.selector.as_slice()) {
        return Err(DecodeError::UnknownInstructionSelector);
    }

    let mut decoder = Decoder::new(&data[instruction.selector.len()..]);
    let mut arguments = Vec::with_capacity(instruction.arguments.len());
    for arg in &instruction.arguments {
        arguments.push(DecodedField {
            name: arg.name.clone(),
            value: decoder.decode_type(defined_types, &arg.ty, &arg.name)?,
        });
    }

    if !decoder.is_finished() {
        return Err(DecodeError::TrailingBytes {
            remaining: decoder.remaining(),
        });
    }

    Ok(DecodedInstruction {
        name: instruction.name.clone(),
        selector: instruction.selector.clone(),
        arguments,
    })
}

impl InstructionSchema {
    fn from_raw(raw: RawInstructionNode) -> Result<Self> {
        let mut selector = None;
        let mut arguments = Vec::with_capacity(raw.arguments.len());

        for arg in raw.arguments {
            if arg.name == "discriminator"
                && arg.default_value_strategy.as_deref() == Some("omitted")
            {
                selector = Some(arg.selector_bytes(&raw.name)?);
                continue;
            }
            arguments.push(ArgumentSchema {
                name: arg.name,
                ty: TypeNode::from_raw(arg.ty, &raw.name)?,
            });
        }

        Ok(Self {
            name: raw.name.clone(),
            selector: selector.ok_or(ParseError::MissingDiscriminator {
                instruction: raw.name,
            })?,
            arguments,
        })
    }
}

impl DefinedTypeSchema {
    fn from_raw(raw: RawDefinedTypeNode) -> Result<Self> {
        Ok(Self {
            name: raw.name.clone(),
            ty: TypeNode::from_raw(raw.ty, &raw.name)?,
        })
    }
}

impl TypeNode {
    fn from_raw(raw: RawTypeNode, context: &str) -> Result<Self> {
        match raw {
            RawTypeNode::NumberTypeNode(node) => {
                Ok(Self::Number(NumberType::from_raw(node, context)?))
            }
            RawTypeNode::BooleanTypeNode { size } => match *size {
                RawTypeNode::NumberTypeNode(node) => Ok(Self::Boolean {
                    size: NumberType::from_raw(node, context)?,
                }),
                other => Err(ParseError::UnsupportedTypeNode {
                    context: context.to_string(),
                    kind: other.kind_name().to_string(),
                }),
            },
            RawTypeNode::PublicKeyTypeNode => Ok(Self::PublicKey),
            RawTypeNode::BytesTypeNode => Ok(Self::Bytes),
            RawTypeNode::StringTypeNode { encoding } => Ok(Self::String {
                encoding: StringEncoding::parse(&encoding, context)?,
            }),
            RawTypeNode::FixedSizeTypeNode { size, item } => Ok(Self::FixedSize {
                size,
                item: Box::new(Self::from_raw(*item, context)?),
            }),
            RawTypeNode::SizePrefixTypeNode { item, prefix } => Ok(Self::SizePrefix {
                item: Box::new(Self::from_raw(*item, context)?),
                prefix: Self::parse_number_node(*prefix, context)?,
            }),
            RawTypeNode::OptionTypeNode {
                fixed,
                item,
                prefix,
            } => Ok(Self::Option {
                fixed,
                item: Box::new(Self::from_raw(*item, context)?),
                prefix: Self::parse_number_node(*prefix, context)?,
            }),
            RawTypeNode::ArrayTypeNode { item, count } => Ok(Self::Array {
                item: Box::new(Self::from_raw(*item, context)?),
                count: CountNode::from_raw(count, context)?,
            }),
            RawTypeNode::StructTypeNode { fields } => Ok(Self::Struct {
                fields: fields
                    .into_iter()
                    .map(|field| StructField::from_raw(field, context))
                    .collect::<Result<Vec<_>>>()?,
            }),
            RawTypeNode::EnumTypeNode { variants, size } => Ok(Self::Enum {
                variants: variants
                    .into_iter()
                    .map(|variant| EnumVariant::from_raw(variant, context))
                    .collect::<Result<Vec<_>>>()?,
                size: Self::parse_number_node(*size, context)?,
            }),
            RawTypeNode::DefinedTypeLinkNode { name } => Ok(Self::Defined(name)),
        }
    }

    fn parse_number_node(raw: RawTypeNode, context: &str) -> Result<NumberType> {
        match raw {
            RawTypeNode::NumberTypeNode(node) => NumberType::from_raw(node, context),
            other => Err(ParseError::UnsupportedTypeNode {
                context: context.to_string(),
                kind: other.kind_name().to_string(),
            }),
        }
    }
}

impl StructField {
    fn from_raw(raw: RawStructFieldTypeNode, context: &str) -> Result<Self> {
        Ok(Self {
            name: raw.name,
            ty: TypeNode::from_raw(raw.ty, context)?,
        })
    }
}

impl EnumVariant {
    fn from_raw(raw: RawEnumVariantTypeNode, context: &str) -> Result<Self> {
        match raw {
            RawEnumVariantTypeNode::EnumEmptyVariantTypeNode {
                name,
                discriminator,
            } => Ok(Self::Empty {
                name,
                discriminator,
            }),
            RawEnumVariantTypeNode::EnumStructVariantTypeNode {
                name,
                discriminator,
                strukt,
            } => Ok(Self::Struct {
                name,
                discriminator,
                fields: strukt
                    .fields
                    .into_iter()
                    .map(|field| StructField::from_raw(field, context))
                    .collect::<Result<Vec<_>>>()?,
            }),
        }
    }
}

impl CountNode {
    fn from_raw(raw: RawCountNode, context: &str) -> Result<Self> {
        match raw {
            RawCountNode::FixedCountNode { value } => Ok(Self::Fixed(value)),
            RawCountNode::PrefixedCountNode { prefix } => Ok(Self::Prefixed(
                TypeNode::parse_number_node(*prefix, context)?,
            )),
        }
    }
}

fn collect_defined_type_names(ty: &TypeNode, pending: &mut Vec<String>) {
    match ty {
        TypeNode::FixedSize { item, .. }
        | TypeNode::SizePrefix { item, .. }
        | TypeNode::Option { item, .. }
        | TypeNode::Array { item, .. } => collect_defined_type_names(item, pending),
        TypeNode::Struct { fields } => {
            for field in fields {
                collect_defined_type_names(&field.ty, pending);
            }
        }
        TypeNode::Enum { variants, .. } => {
            for variant in variants {
                if let EnumVariant::Struct { fields, .. } = variant {
                    for field in fields {
                        collect_defined_type_names(&field.ty, pending);
                    }
                }
            }
        }
        TypeNode::Defined(name) => {
            if !pending.iter().any(|existing| existing == name) {
                pending.push(name.clone());
            }
        }
        TypeNode::Number(_)
        | TypeNode::Boolean { .. }
        | TypeNode::PublicKey
        | TypeNode::Bytes
        | TypeNode::String { .. } => {}
    }
}

struct Decoder<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn is_finished(&self) -> bool {
        self.offset == self.data.len()
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    fn take_bytes(&mut self, len: usize, context: &str) -> DecodeResult<&'a [u8]> {
        let remaining = self.remaining();
        if remaining < len {
            return Err(DecodeError::UnexpectedEof {
                context: context.to_string(),
                needed: len,
                remaining,
            });
        }

        let start = self.offset;
        self.offset += len;
        Ok(&self.data[start..self.offset])
    }

    fn decode_type(
        &mut self,
        defined_types: &[DefinedTypeSchema],
        ty: &TypeNode,
        context: &str,
    ) -> DecodeResult<DecodedValue> {
        match ty {
            TypeNode::Number(number) => {
                Ok(DecodedValue::Number(self.decode_number(*number, context)?))
            }
            TypeNode::Boolean { size } => {
                let value = self.decode_number(*size, context)?;
                match value {
                    DecodedNumber::U8(0) => Ok(DecodedValue::Boolean(false)),
                    DecodedNumber::U8(1) => Ok(DecodedValue::Boolean(true)),
                    other => Err(DecodeError::InvalidBoolValue {
                        context: context.to_string(),
                        value: other,
                    }),
                }
            }
            TypeNode::PublicKey => {
                let bytes = self.take_bytes(32, context)?;
                let mut out = [0u8; 32];
                out.copy_from_slice(bytes);
                Ok(DecodedValue::PublicKey(out))
            }
            TypeNode::Bytes => Err(DecodeError::UnsupportedUnboundedType {
                context: context.to_string(),
                kind: "bytes",
            }),
            TypeNode::String { encoding } => Err(DecodeError::UnsupportedUnboundedType {
                context: format!("{context} ({encoding:?})"),
                kind: "string",
            }),
            TypeNode::FixedSize { size, item } => {
                let bytes = self.take_bytes(*size as usize, context)?;
                let mut nested = Decoder::new(bytes);
                let value = nested.decode_type(defined_types, item, context)?;
                if !nested.is_finished() {
                    return Err(DecodeError::TrailingBytes {
                        remaining: nested.remaining(),
                    });
                }
                Ok(value)
            }
            TypeNode::SizePrefix { item, prefix } => {
                let len = self.decode_usize(*prefix, context)?;
                let bytes = self.take_bytes(len, context)?;
                Self::decode_sized_value(defined_types, item, bytes, context)
            }
            TypeNode::Option { item, prefix, .. } => {
                let tag = self.decode_u64(*prefix, context)?;
                match tag {
                    0 => Ok(DecodedValue::Option(None)),
                    1 => Ok(DecodedValue::Option(Some(Box::new(self.decode_type(
                        defined_types,
                        item,
                        context,
                    )?)))),
                    other => Err(DecodeError::InvalidOptionTag {
                        context: context.to_string(),
                        value: other,
                    }),
                }
            }
            TypeNode::Array { item, count } => {
                let len = match count {
                    CountNode::Fixed(value) => *value as usize,
                    CountNode::Prefixed(prefix) => self.decode_usize(*prefix, context)?,
                };
                let mut values = Vec::with_capacity(len);
                for index in 0..len {
                    let item_context = format!("{context}[{index}]");
                    values.push(self.decode_type(defined_types, item, &item_context)?);
                }
                Ok(DecodedValue::Array(values))
            }
            TypeNode::Struct { fields } => {
                let mut decoded = Vec::with_capacity(fields.len());
                for field in fields {
                    let field_context = format!("{context}.{}", field.name);
                    decoded.push(DecodedField {
                        name: field.name.clone(),
                        value: self.decode_type(defined_types, &field.ty, &field_context)?,
                    });
                }
                Ok(DecodedValue::Struct(decoded))
            }
            TypeNode::Enum { variants, size } => {
                let discriminator = self.decode_u64(*size, context)?;
                let variant = variants
                    .iter()
                    .enumerate()
                    .find(|(index, variant)| match variant {
                        EnumVariant::Empty {
                            discriminator: Some(value),
                            ..
                        }
                        | EnumVariant::Struct {
                            discriminator: Some(value),
                            ..
                        } => *value == discriminator,
                        EnumVariant::Empty {
                            discriminator: None,
                            ..
                        }
                        | EnumVariant::Struct {
                            discriminator: None,
                            ..
                        } => (*index as u64) == discriminator,
                    })
                    .map(|(_, variant)| variant)
                    .ok_or(DecodeError::InvalidEnumDiscriminator {
                        context: context.to_string(),
                        value: discriminator,
                    })?;

                match variant {
                    EnumVariant::Empty { name, .. } => Ok(DecodedValue::Enum(DecodedEnumVariant {
                        name: name.clone(),
                        value: None,
                    })),
                    EnumVariant::Struct { name, fields, .. } => {
                        let mut decoded = Vec::with_capacity(fields.len());
                        for field in fields {
                            let field_context = format!("{context}.{}.{}", name, field.name);
                            decoded.push(DecodedField {
                                name: field.name.clone(),
                                value: self.decode_type(
                                    defined_types,
                                    &field.ty,
                                    &field_context,
                                )?,
                            });
                        }
                        Ok(DecodedValue::Enum(DecodedEnumVariant {
                            name: name.clone(),
                            value: Some(decoded),
                        }))
                    }
                }
            }
            TypeNode::Defined(name) => {
                let defined = defined_types
                    .iter()
                    .find(|defined| defined.name == *name)
                    .ok_or_else(|| DecodeError::UnknownDefinedType(name.clone()))?;
                self.decode_type(defined_types, &defined.ty, context)
            }
        }
    }

    fn decode_sized_value(
        defined_types: &[DefinedTypeSchema],
        item: &TypeNode,
        bytes: &[u8],
        context: &str,
    ) -> DecodeResult<DecodedValue> {
        match item {
            TypeNode::Bytes => Ok(DecodedValue::Bytes(bytes.to_vec())),
            TypeNode::String { encoding } => match encoding {
                StringEncoding::Utf8 => {
                    let text =
                        core::str::from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8 {
                            context: context.to_string(),
                        })?;
                    Ok(DecodedValue::String(text.to_string()))
                }
                _ => Err(DecodeError::UnsupportedUnboundedType {
                    context: context.to_string(),
                    kind: "non-utf8 string",
                }),
            },
            _ => {
                let mut nested = Decoder::new(bytes);
                let value = nested.decode_type(defined_types, item, context)?;
                if !nested.is_finished() {
                    return Err(DecodeError::TrailingBytes {
                        remaining: nested.remaining(),
                    });
                }
                Ok(value)
            }
        }
    }

    fn decode_number(&mut self, number: NumberType, context: &str) -> DecodeResult<DecodedNumber> {
        Ok(match number.format {
            NumberFormat::U8 => DecodedNumber::U8(self.take_bytes(1, context)?[0]),
            NumberFormat::U16 => DecodedNumber::U16(match number.endian {
                NumberEndian::Little => {
                    u16::from_le_bytes(self.take_bytes(2, context)?.try_into().unwrap())
                }
                NumberEndian::Big => {
                    u16::from_be_bytes(self.take_bytes(2, context)?.try_into().unwrap())
                }
            }),
            NumberFormat::U32 => DecodedNumber::U32(match number.endian {
                NumberEndian::Little => {
                    u32::from_le_bytes(self.take_bytes(4, context)?.try_into().unwrap())
                }
                NumberEndian::Big => {
                    u32::from_be_bytes(self.take_bytes(4, context)?.try_into().unwrap())
                }
            }),
            NumberFormat::U64 => DecodedNumber::U64(match number.endian {
                NumberEndian::Little => {
                    u64::from_le_bytes(self.take_bytes(8, context)?.try_into().unwrap())
                }
                NumberEndian::Big => {
                    u64::from_be_bytes(self.take_bytes(8, context)?.try_into().unwrap())
                }
            }),
            NumberFormat::I64 => DecodedNumber::I64(match number.endian {
                NumberEndian::Little => {
                    i64::from_le_bytes(self.take_bytes(8, context)?.try_into().unwrap())
                }
                NumberEndian::Big => {
                    i64::from_be_bytes(self.take_bytes(8, context)?.try_into().unwrap())
                }
            }),
        })
    }

    fn decode_u64(&mut self, number: NumberType, context: &str) -> DecodeResult<u64> {
        match self.decode_number(number, context)? {
            DecodedNumber::U8(value) => Ok(value as u64),
            DecodedNumber::U16(value) => Ok(value as u64),
            DecodedNumber::U32(value) => Ok(value as u64),
            DecodedNumber::U64(value) => Ok(value),
            DecodedNumber::I64(value) if value >= 0 => Ok(value as u64),
            value => Err(DecodeError::InvalidEnumDiscriminator {
                context: context.to_string(),
                value: match value {
                    DecodedNumber::I64(v) => v as u64,
                    _ => 0,
                },
            }),
        }
    }

    fn decode_usize(&mut self, number: NumberType, context: &str) -> DecodeResult<usize> {
        let value = self.decode_u64(number, context)?;
        usize::try_from(value).map_err(|_| DecodeError::InvalidEnumDiscriminator {
            context: context.to_string(),
            value,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawIndexedRootNode<'a> {
    kind: &'a str,
    standard: &'a str,
    #[serde(borrow)]
    program: RawIndexedProgramNode<'a>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIndexedProgramNode<'a> {
    name: String,
    public_key: String,
    version: String,
    origin: Option<String>,
    #[serde(borrow)]
    instructions: Vec<&'a RawValue>,
    #[serde(rename = "definedTypes", borrow)]
    defined_types: Vec<&'a RawValue>,
}

#[derive(Debug, Deserialize)]
struct RawRootNode {
    kind: String,
    standard: String,
    program: RawProgramNode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProgramNode {
    name: String,
    public_key: String,
    version: String,
    origin: Option<String>,
    instructions: Vec<RawInstructionNode>,
    #[serde(rename = "definedTypes")]
    defined_types: Vec<RawDefinedTypeNode>,
}

#[derive(Debug, Deserialize)]
struct RawInstructionNode {
    name: String,
    arguments: Vec<RawInstructionArgumentNode>,
}

#[derive(Debug, Deserialize)]
struct RawInstructionSelectorNode {
    name: String,
    arguments: Vec<RawInstructionSelectorArgumentNode>,
}

impl RawInstructionSelectorNode {
    fn selector_bytes(self) -> Result<Vec<u8>> {
        for argument in self.arguments {
            if argument.name == "discriminator"
                && argument.default_value_strategy.as_deref() == Some("omitted")
            {
                return argument.selector_bytes(&self.name);
            }
        }

        Err(ParseError::MissingDiscriminator {
            instruction: self.name,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawInstructionArgumentNode {
    name: String,
    #[serde(default)]
    default_value_strategy: Option<String>,
    #[serde(rename = "type")]
    ty: RawTypeNode,
    #[serde(default)]
    default_value: Option<RawValueNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawInstructionSelectorArgumentNode {
    name: String,
    #[serde(default)]
    default_value_strategy: Option<String>,
    #[serde(default)]
    default_value: Option<RawValueNode>,
}

impl RawInstructionArgumentNode {
    fn selector_bytes(self, instruction_name: &str) -> Result<Vec<u8>> {
        selector_bytes_from_value(self.default_value, instruction_name)
    }
}

impl RawInstructionSelectorArgumentNode {
    fn selector_bytes(self, instruction_name: &str) -> Result<Vec<u8>> {
        selector_bytes_from_value(self.default_value, instruction_name)
    }
}

fn selector_bytes_from_value(
    default_value: Option<RawValueNode>,
    instruction_name: &str,
) -> Result<Vec<u8>> {
    let value = default_value.ok_or_else(|| ParseError::MissingDiscriminator {
        instruction: instruction_name.to_string(),
    })?;

    match value {
        RawValueNode::BytesValueNode { encoding, data } => {
            if encoding != "base16" {
                return Err(ParseError::InvalidDiscriminatorEncoding {
                    instruction: instruction_name.to_string(),
                    encoding,
                });
            }
            decode_hex(&data).ok_or_else(|| ParseError::InvalidDiscriminatorHex {
                instruction: instruction_name.to_string(),
            })
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawDefinedTypeNode {
    name: String,
    #[serde(rename = "type")]
    ty: RawTypeNode,
}

#[derive(Debug, Deserialize)]
struct RawDefinedTypeNameNode {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum RawValueNode {
    #[serde(rename = "bytesValueNode")]
    BytesValueNode { encoding: String, data: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum RawTypeNode {
    #[serde(rename = "numberTypeNode")]
    NumberTypeNode(RawNumberTypeNode),
    #[serde(rename = "booleanTypeNode")]
    BooleanTypeNode { size: Box<RawTypeNode> },
    #[serde(rename = "publicKeyTypeNode")]
    PublicKeyTypeNode,
    #[serde(rename = "bytesTypeNode")]
    BytesTypeNode,
    #[serde(rename = "stringTypeNode")]
    StringTypeNode { encoding: String },
    #[serde(rename = "fixedSizeTypeNode")]
    FixedSizeTypeNode {
        size: u32,
        #[serde(rename = "type")]
        item: Box<RawTypeNode>,
    },
    #[serde(rename = "sizePrefixTypeNode")]
    SizePrefixTypeNode {
        #[serde(rename = "type")]
        item: Box<RawTypeNode>,
        prefix: Box<RawTypeNode>,
    },
    #[serde(rename = "optionTypeNode")]
    OptionTypeNode {
        #[serde(default)]
        fixed: bool,
        item: Box<RawTypeNode>,
        prefix: Box<RawTypeNode>,
    },
    #[serde(rename = "arrayTypeNode")]
    ArrayTypeNode {
        item: Box<RawTypeNode>,
        count: RawCountNode,
    },
    #[serde(rename = "structTypeNode")]
    StructTypeNode { fields: Vec<RawStructFieldTypeNode> },
    #[serde(rename = "enumTypeNode")]
    EnumTypeNode {
        variants: Vec<RawEnumVariantTypeNode>,
        size: Box<RawTypeNode>,
    },
    #[serde(rename = "definedTypeLinkNode")]
    DefinedTypeLinkNode { name: String },
}

impl RawTypeNode {
    fn kind_name(&self) -> &str {
        match self {
            Self::NumberTypeNode(_) => "numberTypeNode",
            Self::BooleanTypeNode { .. } => "booleanTypeNode",
            Self::PublicKeyTypeNode => "publicKeyTypeNode",
            Self::BytesTypeNode => "bytesTypeNode",
            Self::StringTypeNode { .. } => "stringTypeNode",
            Self::FixedSizeTypeNode { .. } => "fixedSizeTypeNode",
            Self::SizePrefixTypeNode { .. } => "sizePrefixTypeNode",
            Self::OptionTypeNode { .. } => "optionTypeNode",
            Self::ArrayTypeNode { .. } => "arrayTypeNode",
            Self::StructTypeNode { .. } => "structTypeNode",
            Self::EnumTypeNode { .. } => "enumTypeNode",
            Self::DefinedTypeLinkNode { .. } => "definedTypeLinkNode",
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawNumberTypeNode {
    format: String,
    #[serde(default)]
    endian: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum RawCountNode {
    #[serde(rename = "fixedCountNode")]
    FixedCountNode { value: u32 },
    #[serde(rename = "prefixedCountNode")]
    PrefixedCountNode { prefix: Box<RawTypeNode> },
}

#[derive(Debug, Deserialize)]
struct RawStructTypeNode {
    fields: Vec<RawStructFieldTypeNode>,
}

#[derive(Debug, Deserialize)]
struct RawStructFieldTypeNode {
    name: String,
    #[serde(rename = "type")]
    ty: RawTypeNode,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum RawEnumVariantTypeNode {
    #[serde(rename = "enumEmptyVariantTypeNode")]
    EnumEmptyVariantTypeNode {
        name: String,
        #[serde(default)]
        discriminator: Option<u64>,
    },
    #[serde(rename = "enumStructVariantTypeNode")]
    EnumStructVariantTypeNode {
        name: String,
        #[serde(default)]
        discriminator: Option<u64>,
        #[serde(rename = "struct")]
        strukt: RawStructTypeNode,
    },
}

fn decode_hex(input: &str) -> Option<Vec<u8>> {
    if input.len() % 2 != 0 {
        return None;
    }

    let mut out = Vec::with_capacity(input.len() / 2);
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = decode_hex_nibble(bytes[i])?;
        let lo = decode_hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const SQUADS_IDL: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../squads-v4.codama.json"
    ));
    const SQUADS_PRUNED_IDL: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../squads-v4.pruned.codama.json"
    ));

    #[test]
    fn parses_squads_program_metadata() {
        let program = parse_program_schema(SQUADS_IDL).unwrap();
        assert_eq!(program.name, "squadsMultisigProgram");
        assert_eq!(program.version, "2.0.0");
        assert_eq!(program.origin.as_deref(), Some("anchor"));
        assert_eq!(program.instructions.len(), 31);
        assert_eq!(program.defined_types.len(), 14);
    }

    #[test]
    fn parses_pruned_squads_program_metadata() {
        let program = parse_program_schema(SQUADS_PRUNED_IDL).unwrap();
        assert_eq!(program.name, "squadsMultisigProgram");
        assert_eq!(program.version, "2.0.0");
        assert_eq!(program.origin.as_deref(), Some("anchor"));
        assert_eq!(program.instructions.len(), 31);
        assert_eq!(program.defined_types.len(), 5);
        assert_eq!(
            program
                .defined_types
                .iter()
                .map(|defined_type| defined_type.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "proposalVoteArgs",
                "member",
                "permissions",
                "configAction",
                "period",
            ]
        );
    }

    #[test]
    fn extracts_instruction_selector_and_omits_discriminator_arg() {
        let program = parse_program_schema(SQUADS_IDL).unwrap();
        let instruction = program.instruction_by_name("proposalApprove").unwrap();
        assert_eq!(
            instruction.selector,
            decode_hex("9025a488bcd82af8").unwrap()
        );
        assert_eq!(instruction.arguments.len(), 1);
        assert_eq!(instruction.arguments[0].name, "args");
        assert_eq!(
            instruction.arguments[0].ty,
            TypeNode::Defined("proposalVoteArgs".to_string())
        );
    }

    #[test]
    fn parses_nested_instruction_arguments() {
        let program = parse_program_schema(SQUADS_IDL).unwrap();
        let instruction = program
            .instruction_by_name("vaultTransactionCreate")
            .unwrap();
        assert_eq!(instruction.arguments.len(), 4);

        assert_eq!(
            instruction.arguments[0].ty,
            TypeNode::Number(NumberType {
                format: NumberFormat::U8,
                endian: NumberEndian::Little,
            })
        );

        match &instruction.arguments[2].ty {
            TypeNode::SizePrefix { item, prefix } => {
                assert_eq!(**item, TypeNode::Bytes);
                assert_eq!(
                    *prefix,
                    NumberType {
                        format: NumberFormat::U32,
                        endian: NumberEndian::Little,
                    }
                );
            }
            other => panic!("unexpected transactionMessage type: {other:?}"),
        }
    }

    #[test]
    fn parses_defined_enum_types() {
        let program = parse_program_schema(SQUADS_IDL).unwrap();
        let ty = program.defined_type("proposalStatus").unwrap();

        match &ty.ty {
            TypeNode::Enum { variants, size } => {
                assert_eq!(variants.len(), 7);
                assert_eq!(
                    *size,
                    NumberType {
                        format: NumberFormat::U8,
                        endian: NumberEndian::Little,
                    }
                );
                assert!(matches!(
                    &variants[4],
                    EnumVariant::Empty {
                        name,
                        discriminator
                    } if name == "executing" && discriminator.is_none()
                ));
            }
            other => panic!("unexpected proposalStatus type: {other:?}"),
        }
    }

    #[test]
    fn parses_boolean_arguments() {
        let program = parse_program_schema(SQUADS_IDL).unwrap();
        let instruction = program.instruction_by_name("proposalCreate").unwrap();
        let draft = instruction
            .arguments
            .iter()
            .find(|arg| arg.name == "draft")
            .unwrap();

        assert_eq!(
            draft.ty,
            TypeNode::Boolean {
                size: NumberType {
                    format: NumberFormat::U8,
                    endian: NumberEndian::Little,
                }
            }
        );
    }

    #[test]
    fn lazily_loads_and_decodes_proposal_create_instruction_data() {
        let program = parse_program_index(SQUADS_IDL).unwrap();
        let selector = decode_hex("dc3c49e01e6c4f9f").unwrap();
        let bundle = program
            .load_instruction_schema_by_selector(&selector)
            .unwrap()
            .unwrap();

        assert_eq!(bundle.instruction.name, "proposalCreate");
        assert!(bundle.defined_types.is_empty());

        let mut data = selector.clone();
        data.extend_from_slice(&42u64.to_le_bytes());
        data.push(1);

        let decoded = program.decode_instruction_data(&data).unwrap();

        assert_eq!(
            decoded,
            DecodedInstruction {
                name: "proposalCreate".to_string(),
                selector,
                arguments: vec![
                    DecodedField {
                        name: "transactionIndex".to_string(),
                        value: DecodedValue::Number(DecodedNumber::U64(42)),
                    },
                    DecodedField {
                        name: "draft".to_string(),
                        value: DecodedValue::Boolean(true),
                    },
                ],
            }
        );
    }

    #[test]
    fn pruned_idl_decodes_proposal_create_instruction_data() {
        let program = parse_program_index(SQUADS_PRUNED_IDL).unwrap();
        let selector = decode_hex("dc3c49e01e6c4f9f").unwrap();

        let mut data = selector.clone();
        data.extend_from_slice(&42u64.to_le_bytes());
        data.push(1);

        let decoded = program.decode_instruction_data(&data).unwrap();

        assert_eq!(
            decoded,
            DecodedInstruction {
                name: "proposalCreate".to_string(),
                selector,
                arguments: vec![
                    DecodedField {
                        name: "transactionIndex".to_string(),
                        value: DecodedValue::Number(DecodedNumber::U64(42)),
                    },
                    DecodedField {
                        name: "draft".to_string(),
                        value: DecodedValue::Boolean(true),
                    },
                ],
            }
        );
    }
}
