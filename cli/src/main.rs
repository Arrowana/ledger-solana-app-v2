use anyhow::{anyhow, bail, Context, Result};
use bs58::Alphabet;
use clap::{Args, Parser, Subcommand, ValueEnum};
use codama_parser::{
    parse_program_index, DecodedField, DecodedInstruction, DecodedNumber, DecodedValue,
};
use ledger_squads_cli::apdu::{
    build_get_app_config_apdu, build_get_pubkey_apdu, build_list_multisig_slot_apdu,
    build_proposal_create_upgrade_apdu, build_proposal_execute_upgrade_apdu,
    build_proposal_vote_apdu, build_reset_multisigs_apdu, build_save_multisig_apdu,
    build_sign_message_apdus, decode_apdu_response, decode_get_app_config_response,
    decode_get_pubkey_response, decode_list_multisig_slot_response,
    decode_proposal_create_upgrade_response, decode_proposal_execute_upgrade_response,
    decode_proposal_vote_response, decode_save_multisig_response, decode_sign_message_response,
    ProposalCreateUpgradeRequest, ProposalExecuteUpgradeRequest, SavedEntry,
};
use ledger_squads_cli::constants::{
    ProposalVote, TransportKind, MAX_SAVED_MULTISIGS, SW_NOT_FOUND, SW_OK, SW_USER_REFUSED,
};
use ledger_squads_cli::derivation::{format_derivation_path, parse_derivation_path};
use ledger_squads_cli::rpc::RpcClient;
use ledger_squads_cli::transport::{open_transport, DeviceTransport};
use serde_json::{json, Value};
use solana_message_light::{
    AccountRefView, LookupAccountRefView, MessageVersion, MessageView, StaticAccountRefView,
};
use std::fs;

#[derive(Parser)]
#[command(name = "ledger-squads-cli")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    AppConfig(TransportArgs),
    GetPubkey(GetPubkeyArgs),
    SignMessage(SignMessageArgs),
    SaveMultisig(SaveMultisigArgs),
    ListSaved(CommonArgs),
    ProposalVote(ProposalVoteArgs),
    ProposalApprove(ProposalApproveArgs),
    ProposalCreateUpgrade(ProposalCreateUpgradeArgs),
    ProposalExecuteUpgrade(ProposalExecuteUpgradeArgs),
    ResetMultisigs(CommonArgs),
    InspectMessage(InspectMessageArgs),
}

#[derive(Args, Clone)]
struct TransportArgs {
    #[arg(long, env = "LEDGER_SQUADS_TRANSPORT", default_value = "hid")]
    transport: String,
    #[arg(long, env = "SPECULOS_HOST", default_value = "127.0.0.1")]
    speculos_host: String,
    #[arg(long, env = "SPECULOS_APDU_PORT", default_value_t = 9999)]
    speculos_port: u16,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Args, Clone)]
struct CommonArgs {
    #[command(flatten)]
    transport: TransportArgs,
    #[arg(long, env = "LEDGER_SQUADS_NON_CONFIRM", default_value_t = false)]
    unsafe_non_confirm: bool,
}

#[derive(Args)]
struct GetPubkeyArgs {
    #[command(flatten)]
    transport: TransportArgs,
    #[arg(long)]
    derivation_path: String,
    #[arg(long, default_value_t = false)]
    display: bool,
}

#[derive(Args)]
struct SignMessageArgs {
    #[command(flatten)]
    transport: TransportArgs,
    #[arg(long)]
    derivation_path: String,
    #[arg(long)]
    message_hex: String,
}

#[derive(Args)]
struct SaveMultisigArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    rpc_url: Option<String>,
    #[arg(long)]
    unsafe_skip_rpc_checks: bool,
    #[arg(long)]
    multisig: String,
    #[arg(long)]
    derivation_path: String,
}

#[derive(Clone, ValueEnum)]
enum VoteArg {
    Approve,
    Reject,
}

#[derive(Args)]
struct ProposalVoteArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    rpc_url: Option<String>,
    #[arg(long)]
    unsafe_skip_rpc_checks: bool,
    #[arg(long)]
    multisig: String,
    #[arg(long)]
    transaction_index: u64,
    #[arg(long)]
    vote: VoteArg,
    #[arg(long)]
    fee_payer: Option<String>,
    #[arg(long)]
    blockhash: Option<String>,
    #[arg(long, default_value_t = false)]
    send: bool,
}

#[derive(Args)]
struct ProposalApproveArgs {
    #[command(flatten)]
    vote: ProposalVoteArgs,
}

#[derive(Args)]
struct ProposalCreateUpgradeArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    rpc_url: Option<String>,
    #[arg(long)]
    unsafe_skip_rpc_checks: bool,
    #[arg(long)]
    multisig: String,
    #[arg(long)]
    transaction_index: u64,
    #[arg(long)]
    vault_index: u8,
    #[arg(long)]
    program: String,
    #[arg(long)]
    buffer: String,
    #[arg(long)]
    spill: String,
    #[arg(long)]
    transaction_blockhash: Option<String>,
    #[arg(long)]
    proposal_blockhash: Option<String>,
    #[arg(long, default_value_t = false)]
    send: bool,
}

#[derive(Args)]
struct ProposalExecuteUpgradeArgs {
    #[command(flatten)]
    common: CommonArgs,
    #[arg(long)]
    rpc_url: Option<String>,
    #[arg(long)]
    unsafe_skip_rpc_checks: bool,
    #[arg(long)]
    multisig: String,
    #[arg(long)]
    transaction_index: u64,
    #[arg(long)]
    vault_index: u8,
    #[arg(long)]
    program: String,
    #[arg(long)]
    buffer: String,
    #[arg(long)]
    spill: String,
    #[arg(long)]
    blockhash: Option<String>,
    #[arg(long, default_value_t = false)]
    send: bool,
}

#[derive(Args)]
struct InspectMessageArgs {
    #[arg(long)]
    idl: String,
    #[arg(long)]
    message_hex: String,
    #[arg(long)]
    program_id: Option<String>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::AppConfig(args) => handle_app_config(args),
        Commands::GetPubkey(args) => handle_get_pubkey(args),
        Commands::SignMessage(args) => handle_sign_message(args),
        Commands::SaveMultisig(args) => handle_save_multisig(args),
        Commands::ListSaved(args) => handle_list_saved(args),
        Commands::ProposalVote(args) => handle_proposal_vote(args),
        Commands::ProposalApprove(args) => handle_proposal_vote(ProposalVoteArgs {
            vote: VoteArg::Approve,
            ..args.vote
        }),
        Commands::ProposalCreateUpgrade(args) => handle_proposal_create_upgrade(args),
        Commands::ProposalExecuteUpgrade(args) => handle_proposal_execute_upgrade(args),
        Commands::ResetMultisigs(args) => handle_reset(args),
        Commands::InspectMessage(args) => handle_inspect_message(args),
    }
}

fn handle_app_config(args: TransportArgs) -> Result<()> {
    let mut transport = open_transport_from_args(&args)?;
    let response = transport.exchange(&build_get_app_config_apdu()?)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "get app config")?;
    let config = decode_get_app_config_response(data)?;

    print_output(
        args.json,
        json!({
            "blindSigningEnabled": config.blind_signing_enabled,
            "pubkeyDisplayMode": config.pubkey_display_mode,
            "version": format!("{}.{}.{}", config.version[0], config.version[1], config.version[2]),
        }),
    );
    Ok(())
}

fn handle_get_pubkey(args: GetPubkeyArgs) -> Result<()> {
    let derivation_path = parse_derivation_path(&args.derivation_path)?;
    let mut transport = open_transport_from_args(&args.transport)?;
    let response = transport.exchange(&build_get_pubkey_apdu(&derivation_path, args.display)?)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "get pubkey")?;
    let pubkey = decode_get_pubkey_response(data)?;

    print_output(
        args.transport.json,
        json!({
            "derivationPath": format_derivation_path(&derivation_path),
            "pubkey": encode_address(&pubkey),
        }),
    );
    Ok(())
}

fn handle_sign_message(args: SignMessageArgs) -> Result<()> {
    let derivation_path = parse_derivation_path(&args.derivation_path)?;
    let message_bytes =
        hex::decode(&args.message_hex).with_context(|| "invalid --message-hex payload")?;
    let message = MessageView::try_new(&message_bytes)
        .map_err(|err| anyhow!("failed to parse Solana message: {err}"))?;
    let mut transport = open_transport_from_args(&args.transport)?;

    let pubkey_response = transport.exchange(&build_get_pubkey_apdu(&derivation_path, false)?)?;
    let (pubkey_data, pubkey_status) = decode_apdu_response(&pubkey_response)?;
    assert_status(pubkey_status, "get pubkey")?;
    let pubkey = decode_get_pubkey_response(pubkey_data)?;

    let apdus = build_sign_message_apdus(&derivation_path, &message_bytes)?;
    let mut signature = None;
    for (index, apdu) in apdus.iter().enumerate() {
        let response = transport.exchange(apdu)?;
        let (data, status) = decode_apdu_response(&response)?;
        assert_status(status, "sign message")?;
        if index + 1 == apdus.len() {
            signature = Some(decode_sign_message_response(data)?);
        } else if !data.is_empty() {
            bail!("unexpected data in intermediate sign response");
        }
    }

    let signature = signature.context("missing final sign response")?;
    print_output(
        args.transport.json,
        json!({
            "derivationPath": format_derivation_path(&derivation_path),
            "pubkey": encode_address(&pubkey),
            "signature": bs58::encode(signature).with_alphabet(Alphabet::BITCOIN).into_string(),
            "signatureHex": hex_string(&signature),
            "messageVersion": match message.version {
                MessageVersion::Legacy => "legacy",
                MessageVersion::V0 => "v0",
            },
            "instructionCount": message.instruction_count(),
            "addressTableLookupCount": message.address_table_lookup_count(),
        }),
    );
    Ok(())
}

fn handle_save_multisig(args: SaveMultisigArgs) -> Result<()> {
    let multisig = decode_address(&args.multisig)?;
    let derivation_path = parse_derivation_path(&args.derivation_path)?;

    if let Some(url) = &args.rpc_url {
        RpcClient::new(url).validate_multisig(&args.multisig)?;
    } else if !args.unsafe_skip_rpc_checks {
        bail!("--rpc-url is required unless --unsafe-skip-rpc-checks is set");
    }

    let mut transport = open_transport_from_common(&args.common)?;
    let apdu =
        build_save_multisig_apdu(&multisig, &derivation_path, args.common.unsafe_non_confirm)?;
    let response = transport.exchange(&apdu)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "save multisig")?;
    let (slot, member) = decode_save_multisig_response(data)?;

    print_output(
        args.common.transport.json,
        json!({
            "slot": slot,
            "multisig": args.multisig,
            "derivationPath": format_derivation_path(&derivation_path),
            "member": encode_address(&member),
        }),
    );
    Ok(())
}

fn handle_list_saved(args: CommonArgs) -> Result<()> {
    let mut transport = open_transport_from_common(&args)?;
    let mut entries = Vec::new();
    for slot in 0..MAX_SAVED_MULTISIGS {
        let response = transport.exchange(&build_list_multisig_slot_apdu(slot)?)?;
        let (data, status) = decode_apdu_response(&response)?;
        if status == SW_NOT_FOUND {
            continue;
        }
        assert_status(status, &format!("list slot {slot}"))?;
        if let Some(entry) = decode_list_multisig_slot_response(slot, data)? {
            entries.push(entry);
        }
    }

    if args.transport.json {
        print_output(
            args.transport.json,
            json!({
                "entries": entries.iter().map(saved_entry_json).collect::<Vec<_>>()
            }),
        );
    } else if entries.is_empty() {
        println!("entries: []");
    } else {
        for entry in entries {
            println!(
                "slot={} multisig={} member={} path={}",
                entry.slot,
                encode_address(&entry.multisig),
                encode_address(&entry.member),
                format_derivation_path(&entry.path)
            );
        }
    }
    Ok(())
}

fn handle_proposal_vote(args: ProposalVoteArgs) -> Result<()> {
    if args.send {
        bail!("--send is not implemented in the Rust CLI yet");
    }

    let multisig = decode_address(&args.multisig)?;
    let fee_payer = args.fee_payer.as_deref().map(decode_address).transpose()?;
    let blockhash = resolve_blockhash(
        args.rpc_url.as_deref(),
        args.blockhash.as_deref(),
        args.unsafe_skip_rpc_checks,
    )?;

    let mut transport = open_transport_from_common(&args.common)?;
    let entries = load_saved_entries(&mut transport)?;
    let saved = entries
        .into_iter()
        .find(|entry| entry.multisig == multisig)
        .with_context(|| format!("multisig is not saved on the Ledger: {}", args.multisig))?;

    if let Some(fee_payer) = fee_payer {
        if fee_payer != saved.member {
            bail!("fee payer must equal the saved Ledger member signer in v1");
        }
    }

    let apdu = build_proposal_vote_apdu(
        &multisig,
        args.transaction_index,
        match args.vote {
            VoteArg::Approve => ProposalVote::Approve,
            VoteArg::Reject => ProposalVote::Reject,
        },
        &blockhash,
        fee_payer.as_ref(),
        args.common.unsafe_non_confirm,
    )?;
    let response = transport.exchange(&apdu)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "proposal vote")?;
    let signed = decode_proposal_vote_response(data)?;

    print_output(
        args.common.transport.json,
        json!({
            "signature": bs58::encode(signed.signature).with_alphabet(Alphabet::BITCOIN).into_string(),
            "member": encode_address(&signed.member),
            "proposal": encode_address(&signed.proposal),
            "messageHash": hex_string(&signed.message_hash),
        }),
    );
    Ok(())
}

fn handle_proposal_create_upgrade(args: ProposalCreateUpgradeArgs) -> Result<()> {
    if args.send {
        bail!("--send is not implemented in the Rust CLI yet");
    }

    let transaction_blockhash = resolve_blockhash(
        args.rpc_url.as_deref(),
        args.transaction_blockhash.as_deref(),
        args.unsafe_skip_rpc_checks,
    )?;
    let proposal_blockhash = resolve_blockhash(
        args.rpc_url.as_deref(),
        args.proposal_blockhash.as_deref(),
        args.unsafe_skip_rpc_checks,
    )?;
    let mut transport = open_transport_from_common(&args.common)?;
    let response = transport.exchange(&build_proposal_create_upgrade_apdu(
        ProposalCreateUpgradeRequest {
            multisig: &decode_address(&args.multisig)?,
            transaction_index: args.transaction_index,
            vault_index: args.vault_index,
            program: &decode_address(&args.program)?,
            buffer: &decode_address(&args.buffer)?,
            spill: &decode_address(&args.spill)?,
            transaction_blockhash: &transaction_blockhash,
            proposal_blockhash: &proposal_blockhash,
            non_confirm: args.common.unsafe_non_confirm,
        },
    )?)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "proposal create upgrade")?;
    let signed = decode_proposal_create_upgrade_response(data)?;

    print_output(
        args.common.transport.json,
        json!({
            "createSignature": bs58::encode(signed.create_signature).with_alphabet(Alphabet::BITCOIN).into_string(),
            "proposalSignature": bs58::encode(signed.proposal_signature).with_alphabet(Alphabet::BITCOIN).into_string(),
            "intentHash": hex_string(&signed.intent_hash),
            "createMessageHash": hex_string(&signed.create_message_hash),
            "proposalMessageHash": hex_string(&signed.proposal_message_hash),
        }),
    );
    Ok(())
}

fn handle_proposal_execute_upgrade(args: ProposalExecuteUpgradeArgs) -> Result<()> {
    if args.send {
        bail!("--send is not implemented in the Rust CLI yet");
    }

    let blockhash = resolve_blockhash(
        args.rpc_url.as_deref(),
        args.blockhash.as_deref(),
        args.unsafe_skip_rpc_checks,
    )?;
    let mut transport = open_transport_from_common(&args.common)?;
    let response = transport.exchange(&build_proposal_execute_upgrade_apdu(
        ProposalExecuteUpgradeRequest {
            multisig: &decode_address(&args.multisig)?,
            transaction_index: args.transaction_index,
            vault_index: args.vault_index,
            program: &decode_address(&args.program)?,
            buffer: &decode_address(&args.buffer)?,
            spill: &decode_address(&args.spill)?,
            blockhash: &blockhash,
            non_confirm: args.common.unsafe_non_confirm,
        },
    )?)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "proposal execute upgrade")?;
    let signed = decode_proposal_execute_upgrade_response(data)?;

    print_output(
        args.common.transport.json,
        json!({
            "signature": bs58::encode(signed.signature).with_alphabet(Alphabet::BITCOIN).into_string(),
            "intentHash": hex_string(&signed.intent_hash),
            "messageHash": hex_string(&signed.message_hash),
        }),
    );
    Ok(())
}

fn handle_reset(args: CommonArgs) -> Result<()> {
    let mut transport = open_transport_from_common(&args)?;
    let response = transport.exchange(&build_reset_multisigs_apdu(args.unsafe_non_confirm)?)?;
    let (_, status) = decode_apdu_response(&response)?;
    assert_status(status, "reset multisigs")?;
    print_output(args.transport.json, json!({ "ok": true }));
    Ok(())
}

fn handle_inspect_message(args: InspectMessageArgs) -> Result<()> {
    let idl_bytes =
        fs::read(&args.idl).with_context(|| format!("failed to read IDL: {}", args.idl))?;
    let program = parse_program_index(&idl_bytes)
        .map_err(|err| anyhow!("failed to parse Codama IDL: {err}"))?;
    let message_bytes =
        hex::decode(&args.message_hex).with_context(|| "invalid --message-hex payload")?;
    let message = MessageView::try_new(&message_bytes)
        .map_err(|err| anyhow!("failed to parse Solana message: {err}"))?;

    let expected_program_id = if let Some(program_id) = args.program_id.as_deref() {
        Some(decode_address(program_id)?)
    } else if !program.public_key.is_empty() {
        Some(
            decode_address(&program.public_key)
                .with_context(|| format!("invalid program id in IDL: {}", program.public_key))?,
        )
    } else {
        None
    };

    let mut instructions = Vec::with_capacity(message.instruction_count());
    for instruction in message.instructions() {
        let instruction =
            instruction.map_err(|err| anyhow!("failed to iterate instruction: {err}"))?;
        let program_ref = message
            .account_ref(instruction.program_id_index)
            .map_err(|err| anyhow!("failed to resolve instruction program: {err}"))?;
        let should_decode = expected_program_id
            .as_ref()
            .map(|expected| account_ref_matches_pubkey(program_ref, expected))
            .unwrap_or(false);

        let decoded = if should_decode {
            Some(
                program
                    .decode_instruction_data(instruction.data)
                    .map_err(|err| anyhow!("failed to decode instruction data: {err}"))?,
            )
        } else {
            None
        };

        let mut accounts = Vec::with_capacity(instruction.account_indexes.len());
        for account_index in instruction.account_indexes {
            accounts.push(account_ref_json(
                message
                    .account_ref(*account_index)
                    .map_err(|err| anyhow!("failed to resolve instruction account: {err}"))?,
            ));
        }

        instructions.push(json!({
            "index": instruction.index,
            "program": account_ref_json(program_ref),
            "accounts": accounts,
            "dataHex": hex_string(instruction.data),
            "decoded": decoded.as_ref().map(decoded_instruction_json),
        }));
    }

    let value = json!({
        "version": match message.version {
            MessageVersion::Legacy => "legacy",
            MessageVersion::V0 => "v0",
        },
        "staticAccountCount": message.static_account_count(),
        "addressTableLookupCount": message.address_table_lookup_count(),
        "instructionCount": message.instruction_count(),
        "instructions": instructions,
    });

    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}

fn open_transport_from_common(
    args: &CommonArgs,
) -> Result<ledger_squads_cli::transport::Transport> {
    open_transport_from_args(&args.transport)
}

fn open_transport_from_args(
    args: &TransportArgs,
) -> Result<ledger_squads_cli::transport::Transport> {
    let kind = TransportKind::parse(&args.transport)?;
    open_transport(kind, &args.speculos_host, args.speculos_port)
}

fn load_saved_entries(transport: &mut impl DeviceTransport) -> Result<Vec<SavedEntry>> {
    let mut entries = Vec::new();
    for slot in 0..MAX_SAVED_MULTISIGS {
        let response = transport.exchange(&build_list_multisig_slot_apdu(slot)?)?;
        let (data, status) = decode_apdu_response(&response)?;
        if status == SW_NOT_FOUND {
            continue;
        }
        assert_status(status, &format!("list slot {slot}"))?;
        if let Some(entry) = decode_list_multisig_slot_response(slot, data)? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn decode_address(value: &str) -> Result<[u8; 32]> {
    let bytes = bs58::decode(value)
        .with_alphabet(Alphabet::BITCOIN)
        .into_vec()
        .with_context(|| format!("invalid base58 value: {value}"))?;
    if bytes.len() != 32 {
        bail!("expected 32-byte address: {value}");
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn encode_address(value: &[u8; 32]) -> String {
    bs58::encode(value)
        .with_alphabet(Alphabet::BITCOIN)
        .into_string()
}

fn resolve_blockhash(
    rpc_url: Option<&str>,
    explicit: Option<&str>,
    allow_skip: bool,
) -> Result<[u8; 32]> {
    let blockhash = if let Some(explicit) = explicit {
        explicit.to_owned()
    } else if let Some(url) = rpc_url {
        RpcClient::new(url).latest_blockhash()?
    } else if allow_skip {
        bail!("a blockhash is required when --rpc-url is omitted");
    } else {
        bail!("--rpc-url is required unless --unsafe-skip-rpc-checks is set");
    };
    decode_address(&blockhash).map_err(|_| anyhow!("invalid blockhash: {blockhash}"))
}

fn assert_status(status: u16, action: &str) -> Result<()> {
    match status {
        SW_OK => Ok(()),
        SW_USER_REFUSED => bail!("user refused {action}"),
        other => bail!("{action} failed with status 0x{other:04x}"),
    }
}

fn saved_entry_json(entry: &SavedEntry) -> Value {
    json!({
        "slot": entry.slot,
        "multisig": encode_address(&entry.multisig),
        "member": encode_address(&entry.member),
        "derivationPath": format_derivation_path(&entry.path),
    })
}

fn account_ref_matches_pubkey(account: AccountRefView<'_>, expected: &[u8; 32]) -> bool {
    match account {
        AccountRefView::Static(account) => account.pubkey == expected,
        AccountRefView::Lookup(_) => false,
    }
}

fn account_ref_json(account: AccountRefView<'_>) -> Value {
    match account {
        AccountRefView::Static(StaticAccountRefView {
            global_index,
            pubkey,
            signer,
            writable,
        }) => json!({
            "kind": "static",
            "index": global_index,
            "pubkey": encode_address(pubkey),
            "signer": signer,
            "writable": writable,
        }),
        AccountRefView::Lookup(LookupAccountRefView {
            global_index,
            table_account,
            table_index,
            writable,
        }) => json!({
            "kind": "lookup",
            "index": global_index,
            "tableAccount": encode_address(table_account),
            "tableIndex": table_index,
            "writable": writable,
        }),
    }
}

fn decoded_instruction_json(instruction: &DecodedInstruction) -> Value {
    json!({
        "name": instruction.name,
        "selectorHex": hex_string(&instruction.selector),
        "arguments": instruction.arguments.iter().map(decoded_field_json).collect::<Vec<_>>(),
    })
}

fn decoded_field_json(field: &DecodedField) -> Value {
    json!({
        "name": field.name,
        "value": decoded_value_json(&field.value),
    })
}

fn decoded_value_json(value: &DecodedValue) -> Value {
    match value {
        DecodedValue::Number(number) => match number {
            DecodedNumber::U8(value) => json!(value),
            DecodedNumber::U16(value) => json!(value),
            DecodedNumber::U32(value) => json!(value),
            DecodedNumber::U64(value) => json!(value),
            DecodedNumber::I64(value) => json!(value),
        },
        DecodedValue::Boolean(value) => json!(value),
        DecodedValue::PublicKey(value) => json!(encode_address(value)),
        DecodedValue::Bytes(value) => json!({ "hex": hex_string(value) }),
        DecodedValue::String(value) => json!(value),
        DecodedValue::Option(value) => match value {
            Some(value) => decoded_value_json(value),
            None => Value::Null,
        },
        DecodedValue::Array(values) => {
            Value::Array(values.iter().map(decoded_value_json).collect::<Vec<_>>())
        }
        DecodedValue::Struct(fields) => {
            Value::Array(fields.iter().map(decoded_field_json).collect::<Vec<_>>())
        }
        DecodedValue::Enum(variant) => json!({
            "variant": variant.name,
            "fields": variant
                .value
                .as_ref()
                .map(|fields| fields.iter().map(decoded_field_json).collect::<Vec<_>>())
                .unwrap_or_default(),
        }),
    }
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_output(as_json: bool, value: Value) {
    if as_json {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    match value {
        Value::Object(map) => {
            for (key, value) in map {
                println!("{key}: {}", render_value(&value));
            }
        }
        other => println!("{}", render_value(&other)),
    }
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        _ => serde_json::to_string_pretty(value).unwrap(),
    }
}
