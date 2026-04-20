use alloc::vec::Vec;
use codama_parser::{parse_program_index, DecodedInstruction, ProgramIndex};

pub struct BuiltinDecodedInstruction {
    pub program_name: &'static str,
    pub instruction: DecodedInstruction,
}

struct BuiltinIdlSource {
    name: &'static str,
    bytes: &'static [u8],
}

pub struct LoadedBuiltinIdl {
    pub name: &'static str,
    pub program_id: [u8; 32],
    program: ProgramIndex<'static>,
}

const BUILTIN_IDLS: &[BuiltinIdlSource] = &[
    BuiltinIdlSource {
        name: "system",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idls/system.codama.json"
        )),
    },
    BuiltinIdlSource {
        name: "compute-budget",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idls/compute-budget.codama.json"
        )),
    },
    BuiltinIdlSource {
        name: "associated-token-account",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idls/associated-token-account.codama.json"
        )),
    },
    BuiltinIdlSource {
        name: "token",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../idls/token.codama.json"
        )),
    },
];

pub fn decode_instruction(
    loaded_idls: &[LoadedBuiltinIdl],
    program_id: &[u8; 32],
    instruction_data: &[u8],
) -> Option<BuiltinDecodedInstruction> {
    let builtin = loaded_idls
        .iter()
        .find(|builtin| &builtin.program_id == program_id)?;
    let instruction = builtin
        .program
        .decode_instruction_data(instruction_data)
        .ok()?;

    Some(BuiltinDecodedInstruction {
        program_name: builtin.name,
        instruction,
    })
}

pub fn load_builtin_idls() -> Vec<LoadedBuiltinIdl> {
    let mut loaded = Vec::with_capacity(BUILTIN_IDLS.len());
    for builtin in BUILTIN_IDLS {
        if let Some(loaded_idl) = load_builtin_idl(builtin) {
            loaded.push(loaded_idl);
        }
    }
    loaded
}

fn load_builtin_idl(builtin: &'static BuiltinIdlSource) -> Option<LoadedBuiltinIdl> {
    let program = parse_program_index(builtin.bytes).ok()?;
    let program_id = decode_program_id(&program.public_key)?;
    Some(LoadedBuiltinIdl {
        name: builtin.name,
        program_id,
        program,
    })
}

fn decode_program_id(program_id: &str) -> Option<[u8; 32]> {
    let bytes = bs58::decode(program_id).into_vec().ok()?;
    if bytes.len() != 32 {
        return None;
    }

    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
}
