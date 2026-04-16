use anyhow::{anyhow, bail, Context, Result};
use bs58::Alphabet;
use clap::{Args, Parser, Subcommand, ValueEnum};
use ledger_squads_cli::apdu::{
    build_list_multisig_slot_apdu, build_proposal_create_upgrade_apdu,
    build_proposal_execute_upgrade_apdu, build_proposal_vote_apdu, build_reset_multisigs_apdu,
    build_save_multisig_apdu, decode_apdu_response, decode_list_multisig_slot_response,
    decode_proposal_create_upgrade_response, decode_proposal_execute_upgrade_response,
    decode_proposal_vote_response, decode_save_multisig_response, ProposalCreateUpgradeRequest,
    ProposalExecuteUpgradeRequest, SavedEntry,
};
use ledger_squads_cli::constants::{
    ProposalVote, TransportKind, MAX_SAVED_MULTISIGS, SW_NOT_FOUND, SW_OK, SW_USER_REFUSED,
};
use ledger_squads_cli::derivation::{format_derivation_path, parse_derivation_path};
use ledger_squads_cli::rpc::RpcClient;
use ledger_squads_cli::transport::{open_transport, DeviceTransport};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "ledger-squads-cli")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    SaveMultisig(SaveMultisigArgs),
    ListSaved(CommonArgs),
    ProposalVote(ProposalVoteArgs),
    ProposalApprove(ProposalApproveArgs),
    ProposalCreateUpgrade(ProposalCreateUpgradeArgs),
    ProposalExecuteUpgrade(ProposalExecuteUpgradeArgs),
    ResetMultisigs(CommonArgs),
}

#[derive(Args, Clone)]
struct CommonArgs {
    #[arg(long, env = "LEDGER_SQUADS_TRANSPORT", default_value = "hid")]
    transport: String,
    #[arg(long, env = "SPECULOS_HOST", default_value = "127.0.0.1")]
    speculos_host: String,
    #[arg(long, env = "SPECULOS_APDU_PORT", default_value_t = 9999)]
    speculos_port: u16,
    #[arg(long, env = "LEDGER_SQUADS_NON_CONFIRM", default_value_t = false)]
    unsafe_non_confirm: bool,
    #[arg(long, default_value_t = false)]
    json: bool,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
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
    }
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
    let apdu = build_save_multisig_apdu(&multisig, &derivation_path, args.common.unsafe_non_confirm)?;
    let response = transport.exchange(&apdu)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "save multisig")?;
    let (slot, member) = decode_save_multisig_response(data)?;

    print_output(
        &args.common,
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

    if args.json {
        print_output(
            &args,
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
    let fee_payer = args
        .fee_payer
        .as_deref()
        .map(decode_address)
        .transpose()?;
    let blockhash = resolve_blockhash(args.rpc_url.as_deref(), args.blockhash.as_deref(), args.unsafe_skip_rpc_checks)?;

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
        &args.common,
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
        &args.common,
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

    let blockhash = resolve_blockhash(args.rpc_url.as_deref(), args.blockhash.as_deref(), args.unsafe_skip_rpc_checks)?;
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
        &args.common,
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
    print_output(&args, json!({ "ok": true }));
    Ok(())
}

fn open_transport_from_common(args: &CommonArgs) -> Result<ledger_squads_cli::transport::Transport> {
    let kind = TransportKind::parse(&args.transport)?;
    open_transport(kind, &args.speculos_host, args.speculos_port)
}

fn load_saved_entries(
    transport: &mut impl DeviceTransport,
) -> Result<Vec<SavedEntry>> {
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

fn resolve_blockhash(rpc_url: Option<&str>, explicit: Option<&str>, allow_skip: bool) -> Result<[u8; 32]> {
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

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_output(common: &CommonArgs, value: Value) {
    if common.json {
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

