use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure, Context, Result};
use bs58::Alphabet;
use clap::{Args, Parser, Subcommand, ValueEnum};
use ledger_solana_cli::apdu::{
    build_get_app_config_apdu, build_get_pubkey_apdu, build_load_idl_apdus,
    build_sign_message_apdus, decode_apdu_response, decode_get_app_config_response,
    decode_get_pubkey_response, decode_load_idl_response, decode_sign_message_response,
    IdlAttestation,
};
use ledger_solana_cli::constants::{SW_OK, SW_USER_REFUSED};
use ledger_solana_cli::derivation::parse_derivation_path;
use ledger_solana_cli::transport::{DeviceTransport, SpeculosTransport};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use solana_address::Address;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_instruction::Instruction;
use solana_message::Message;
use solana_system_interface::instruction as system_instruction;
use spl_associated_token_account_interface::instruction as associated_token_instruction;
use spl_token_interface::instruction as token_instruction;

const DEFAULT_DERIVATION_PATH: &str = "m/44'/501'/0'/0'";
const DEFAULT_SPECULOS_API_PORT: u16 = 5001;
const DEFAULT_SPECULOS_APDU_PORT: u16 = 9999;
const DEFAULT_SPECULOS_VNC_PORT: u16 = 5900;
const SCREEN_TIMEOUT: Duration = Duration::from_secs(20);
const SCREEN_CHANGE_TIMEOUT: Duration = Duration::from_secs(5);
const SCROLLER_SCROLL_TIMEOUT: Duration = Duration::from_millis(900);
const SIGN_RESULT_TIMEOUT: Duration = Duration::from_secs(30);
const HOME_SCREEN_TITLE: &str = "Solana v2";
const HOME_SCREEN_READY: &str = "app is ready";
const IMPORTED_SAMPLE_PROGRAM_ID: Address =
    Address::from_str_const("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
const IMPORTED_SAMPLE_IDL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../testdata/sample-program.pruned.codama.json"
));
const IMPORTED_SAMPLE_SIGNER_PUBKEY_HEX: &str =
    "99a191c52aefd12a1c6fca50c4a71d3a51e596deacbedd5a539a2c57ca5e7dc4";
const IMPORTED_SAMPLE_SIGNATURE_HEX: &str =
    "b7a73aa8a9bcb18883f4d75b577ec0511577050641ab61c747c6801523c123e214d0992524faf68b43be6e6dee7111b61df654f3d26e34f732edbe356558ae0c";
const NO_FORBIDDEN_FRAGMENTS: &[&str] = &[];

#[derive(Parser)]
#[command(name = "app-integration-tests")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    SpeculosSmoke(SpeculosSmokeArgs),
    ReviewLoadIdl(ReviewLoadIdlArgs),
}

#[derive(Args, Clone)]
struct SpeculosSmokeArgs {
    #[arg(long, default_value_t = false)]
    skip_build: bool,
    #[arg(long, default_value_t = false)]
    manual_review: bool,
    #[arg(long, default_value_t = false)]
    manual_load_idl_review: bool,
    #[arg(long, default_value = DEFAULT_DERIVATION_PATH)]
    derivation_path: String,
    #[arg(long)]
    api_port: Option<u16>,
    #[arg(long)]
    apdu_port: Option<u16>,
    #[arg(long)]
    vnc_port: Option<u16>,
    #[arg(long, value_enum)]
    cases: Vec<SmokeCaseName>,
}

#[derive(Args, Clone)]
struct ReviewLoadIdlArgs {
    #[arg(long, default_value_t = false)]
    skip_build: bool,
    #[arg(long, default_value = DEFAULT_DERIVATION_PATH)]
    derivation_path: String,
    #[arg(long)]
    api_port: Option<u16>,
    #[arg(long)]
    apdu_port: Option<u16>,
    #[arg(long)]
    vnc_port: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SmokeCaseName {
    SystemTransfer,
    ComputeBudgetLimit,
    AtaCreate,
    TokenTransfer,
    ImportedProgramGeneric,
    ImportedProgramDecoded,
}

impl SmokeCaseName {
    fn all() -> [Self; 4] {
        [
            Self::SystemTransfer,
            Self::ComputeBudgetLimit,
            Self::AtaCreate,
            Self::TokenTransfer,
        ]
    }

    fn slug(self) -> &'static str {
        match self {
            Self::SystemTransfer => "system-transfer",
            Self::ComputeBudgetLimit => "compute-budget-limit",
            Self::AtaCreate => "ata-create",
            Self::TokenTransfer => "token-transfer",
            Self::ImportedProgramGeneric => "imported-program-generic",
            Self::ImportedProgramDecoded => "imported-program-decoded",
        }
    }
}

#[derive(Clone, Copy)]
struct ExpectedScreen {
    fragments: &'static [&'static str],
}

struct SmokeCase {
    name: SmokeCaseName,
    message: Vec<u8>,
    message_hash: String,
    review_section_count: usize,
    expected_screens: &'static [ExpectedScreen],
    forbidden_fragments: &'static [&'static str],
}

const MESSAGE_HASH_SCREEN: ExpectedScreen = ExpectedScreen {
    fragments: &["Message", "256"],
};

const SYSTEM_TRANSFER_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["1/1", "system", "transferSol"],
    },
    ExpectedScreen {
        fragments: &["amount", "42_000_000_000"],
    },
    ExpectedScreen {
        fragments: &["source", "<wallet>"],
    },
];

const COMPUTE_BUDGET_LIMIT_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["1/1", "compute-budget", "setComputeUnitLimi"],
    },
    ExpectedScreen {
        fragments: &["units", "1_400_000"],
    },
];

const ATA_CREATE_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["1/1", "associated-token", "create"],
    },
    ExpectedScreen {
        fragments: &["funder", "<wallet>"],
    },
];

const TOKEN_TRANSFER_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["1/1", "token", "transfer"],
    },
    ExpectedScreen {
        fragments: &["amount", "42_000_000"],
    },
    ExpectedScreen {
        fragments: &["authority", "<wallet>"],
    },
];

const UNKNOWN_PROGRAM_GENERIC_SCREENS: &[ExpectedScreen] = &[ExpectedScreen {
    fragments: &["dataLen", "17 bytes"],
}];

const UNKNOWN_PROGRAM_GENERIC_FORBIDDEN: &[&str] = &[
    "reviewRequest",
    "requestIndex",
    "urgent",
    "sampleOperations",
];

const IMPORTED_PROGRAM_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["1/1", "sampleOperations", "reviewRequest"],
    },
    ExpectedScreen {
        fragments: &["request", "42"],
    },
    ExpectedScreen {
        fragments: &["urgent", "true"],
    },
];

struct Ports {
    api: u16,
    apdu: u16,
    vnc: u16,
}

struct SpeculosProcess {
    child: Child,
}

impl SpeculosProcess {
    fn spawn(root: &Path, ports: &Ports) -> Result<Self> {
        let child = Command::new("bash")
            .arg("./scripts/run-speculos.sh")
            .current_dir(root)
            .env("SPECULOS_API_PORT", ports.api.to_string())
            .env("SPECULOS_APDU_PORT", ports.apdu.to_string())
            .env("SPECULOS_VNC_PORT", ports.vnc.to_string())
            .env("SPECULOS_DISPLAY", "headless")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to launch Speculos")?;
        Ok(Self { child })
    }
}

impl Drop for SpeculosProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct SpeculosApi {
    base_url: String,
    client: Client,
}

impl SpeculosApi {
    fn new(api_port: u16) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .context("failed to build Speculos API client")?;
        Ok(Self {
            base_url: format!("http://127.0.0.1:{api_port}"),
            client,
        })
    }

    fn current_screen(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/events?currentscreenonly=true", self.base_url))
            .send()
            .context("failed to query Speculos screen")?
            .error_for_status()
            .context("Speculos screen API returned error")?
            .json::<ScreenResponse>()
            .context("failed to parse Speculos screen response")?;

        let mut lines = Vec::new();
        for event in response.events.unwrap_or_default() {
            if let Some(text) = event.text {
                if !lines.iter().any(|line| line == &text) {
                    lines.push(text);
                }
            }
        }
        Ok(lines)
    }

    fn press_button(&self, button: &str) -> Result<()> {
        self.client
            .post(format!("{}/button/{button}", self.base_url))
            .json(&json!({ "action": "press-and-release" }))
            .send()
            .with_context(|| format!("failed to press Speculos {button} button"))?
            .error_for_status()
            .with_context(|| format!("Speculos {button} button API returned error"))?;
        Ok(())
    }

    fn wait_for_screen_contains(&self, fragment: &str, timeout: Duration) -> Result<Vec<String>> {
        let started = Instant::now();
        loop {
            match self.current_screen() {
                Ok(screen) if screen_contains(&screen, fragment) => return Ok(screen),
                Ok(_) | Err(_) if started.elapsed() < timeout => {
                    thread::sleep(Duration::from_millis(150));
                }
                Ok(screen) => {
                    bail!(
                        "timed out waiting for screen containing {fragment:?}; last screen: {}",
                        render_screen(&screen)
                    );
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("timed out waiting for screen containing {fragment:?}")
                    });
                }
            }
        }
    }

    fn wait_for_screen_change(
        &self,
        previous: &[String],
        timeout: Duration,
    ) -> Result<Vec<String>> {
        let started = Instant::now();
        loop {
            let current = self.current_screen()?;
            if !current.is_empty() && current != previous {
                return Ok(current);
            }
            if started.elapsed() >= timeout {
                bail!(
                    "timed out waiting for screen change; current screen: {}",
                    render_screen(&current)
                );
            }
            thread::sleep(Duration::from_millis(150));
        }
    }

    fn try_wait_for_screen_change(
        &self,
        previous: &[String],
        timeout: Duration,
    ) -> Result<Option<Vec<String>>> {
        let started = Instant::now();
        loop {
            let current = self.current_screen()?;
            if !current.is_empty() && current != previous {
                return Ok(Some(current));
            }
            if started.elapsed() >= timeout {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(150));
        }
    }
}

#[derive(Debug, Deserialize)]
struct ScreenEvent {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScreenResponse {
    events: Option<Vec<ScreenEvent>>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::SpeculosSmoke(args) => run_speculos_smoke(args),
        Commands::ReviewLoadIdl(args) => run_review_load_idl(args),
    }
}

fn run_speculos_smoke(args: SpeculosSmokeArgs) -> Result<()> {
    let root = repo_root()?;
    let ports = resolve_ports(args.api_port, args.apdu_port, args.vnc_port);
    let derivation_path = parse_derivation_path(&args.derivation_path)?;
    let cases = selected_cases(&args);

    let (_speculos, api) = launch_speculos(&root, &ports, args.skip_build)?;

    let app_config = read_app_config(ports.apdu)?;
    println!(
        "==> app-config version={}.{}.{} blind_signing={} display_mode={}",
        app_config.version[0],
        app_config.version[1],
        app_config.version[2],
        app_config.blind_signing_enabled,
        app_config.pubkey_display_mode
    );

    let pubkey = read_pubkey(ports.apdu, derivation_path.as_slice())?;
    println!(
        "==> get-pubkey {}",
        bs58::encode(pubkey)
            .with_alphabet(Alphabet::BITCOIN)
            .into_string()
    );

    for case in cases {
        let smoke_case = build_case(case, pubkey)?;
        run_sign_case(
            &api,
            ports.apdu,
            derivation_path.as_slice(),
            &smoke_case,
            args.manual_review,
        )?;
    }

    run_imported_idl_flow(
        &api,
        ports.apdu,
        derivation_path.as_slice(),
        pubkey,
        args.manual_review,
        args.manual_load_idl_review,
    )?;

    println!("==> Speculos smoke tests completed successfully");
    Ok(())
}

fn run_review_load_idl(args: ReviewLoadIdlArgs) -> Result<()> {
    let root = repo_root()?;
    let ports = resolve_ports(args.api_port, args.apdu_port, args.vnc_port);
    let derivation_path = parse_derivation_path(&args.derivation_path)?;
    let (_speculos, api) = launch_speculos(&root, &ports, args.skip_build)?;
    let attestation = imported_sample_attestation()?;

    println!("==> Running standalone load-idl review");
    let response = run_load_idl_case(&api, ports.apdu, IMPORTED_SAMPLE_IDL, &[attestation], true)?;

    ensure!(
        response.program_id == IMPORTED_SAMPLE_PROGRAM_ID.to_bytes(),
        "load-idl returned wrong program id: {}",
        bs58::encode(response.program_id)
            .with_alphabet(Alphabet::BITCOIN)
            .into_string()
    );
    ensure!(
        response.signer_count == 1,
        "load-idl returned wrong signer count: {}",
        response.signer_count
    );
    ensure!(
        response.idl_len as usize == IMPORTED_SAMPLE_IDL.len(),
        "load-idl returned wrong idl length: {}",
        response.idl_len
    );
    println!(
        "==> load-idl imported {} with {} signer",
        bs58::encode(response.program_id)
            .with_alphabet(Alphabet::BITCOIN)
            .into_string(),
        response.signer_count
    );
    let signer_pubkey = read_pubkey(ports.apdu, derivation_path.as_slice())?;
    let decoded_case = build_imported_sample_case(signer_pubkey, true)?;
    run_sign_case(
        &api,
        ports.apdu,
        derivation_path.as_slice(),
        &decoded_case,
        false,
    )?;
    println!("==> Standalone load-idl review completed successfully");
    Ok(())
}

fn read_app_config(apdu_port: u16) -> Result<ledger_solana_cli::apdu::AppConfigResponse> {
    let response = exchange_apdu(apdu_port, &build_get_app_config_apdu()?)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "app-config")?;
    decode_get_app_config_response(data).context("failed to decode app-config response")
}

fn launch_speculos(
    root: &Path,
    ports: &Ports,
    skip_build: bool,
) -> Result<(SpeculosProcess, SpeculosApi)> {
    if !skip_build {
        println!("==> Building Ledger app with scripts/build-ledger.sh");
        run_checked(
            Command::new("bash")
                .arg("./scripts/build-ledger.sh")
                .current_dir(root),
            "scripts/build-ledger.sh",
        )?;
    }

    println!(
        "==> Launching Speculos on api={}, apdu={}, vnc={}",
        ports.api, ports.apdu, ports.vnc
    );
    let speculos = SpeculosProcess::spawn(root, ports)?;
    let api = SpeculosApi::new(ports.api)?;
    api.wait_for_screen_contains(HOME_SCREEN_TITLE, SCREEN_TIMEOUT)?;
    api.wait_for_screen_contains(HOME_SCREEN_READY, SCREEN_TIMEOUT)?;
    println!("==> Speculos is ready");
    Ok((speculos, api))
}

fn read_pubkey(apdu_port: u16, derivation_path: &[u32]) -> Result<[u8; 32]> {
    let response = exchange_apdu(apdu_port, &build_get_pubkey_apdu(derivation_path, false)?)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "get-pubkey")?;
    decode_get_pubkey_response(data).context("failed to decode get-pubkey response")
}

fn run_imported_idl_flow(
    api: &SpeculosApi,
    apdu_port: u16,
    derivation_path: &[u32],
    signer_pubkey: [u8; 32],
    manual_review: bool,
    manual_load_idl_review: bool,
) -> Result<()> {
    println!("==> Running imported IDL flow");

    let generic_case = build_imported_sample_case(signer_pubkey, false)?;
    run_sign_case(
        api,
        apdu_port,
        derivation_path,
        &generic_case,
        manual_review,
    )?;

    let attestation = imported_sample_attestation()?;
    let load_response = run_load_idl_case(
        api,
        apdu_port,
        IMPORTED_SAMPLE_IDL,
        &[attestation],
        manual_load_idl_review,
    )?;
    ensure!(
        load_response.program_id == IMPORTED_SAMPLE_PROGRAM_ID.to_bytes(),
        "load-idl returned wrong program id: {}",
        bs58::encode(load_response.program_id)
            .with_alphabet(Alphabet::BITCOIN)
            .into_string()
    );
    ensure!(
        load_response.signer_count == 1,
        "load-idl returned wrong signer count: {}",
        load_response.signer_count
    );
    ensure!(
        load_response.idl_len as usize == IMPORTED_SAMPLE_IDL.len(),
        "load-idl returned wrong idl length: {}",
        load_response.idl_len
    );
    println!(
        "==> load-idl imported {} with {} signer",
        bs58::encode(load_response.program_id)
            .with_alphabet(Alphabet::BITCOIN)
            .into_string(),
        load_response.signer_count
    );

    let decoded_case = build_imported_sample_case(signer_pubkey, true)?;
    run_sign_case(
        api,
        apdu_port,
        derivation_path,
        &decoded_case,
        manual_review,
    )?;

    let mut invalid_attestation = attestation;
    invalid_attestation.signature[0] ^= 0x01;
    let status = load_idl_expect_status(apdu_port, IMPORTED_SAMPLE_IDL, &[invalid_attestation])?;
    ensure!(
        status == 0x6a80,
        "invalid load-idl should fail with 0x6a80, got 0x{status:04x}"
    );
    println!("==> invalid load-idl signature rejected with 0x{status:04x}");

    run_sign_case(
        api,
        apdu_port,
        derivation_path,
        &decoded_case,
        manual_review,
    )?;
    Ok(())
}

fn run_sign_case(
    api: &SpeculosApi,
    apdu_port: u16,
    derivation_path: &[u32],
    case: &SmokeCase,
    manual_review: bool,
) -> Result<()> {
    println!("==> Running case {}", case.name.slug());
    println!("    Message SHA-256: {}", case.message_hash);
    let message = case.message.clone();
    let path = derivation_path.to_vec();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = sign_message(apdu_port, path.as_slice(), message.as_slice());
        let _ = tx.send(result);
    });

    let home_screen = api.current_screen()?;
    let mut current = api.wait_for_screen_change(&home_screen, SCREEN_TIMEOUT)?;

    if manual_review {
        println!("==> Manual review enabled for {}", case.name.slug());
        println!("    Continue in the Speculos web UI: {}", api.base_url);
        println!("    Message SHA-256: {}", case.message_hash);
        println!("    Expected decoded screens:");
        for expected in case.expected_screens {
            println!("      - {}", expected.fragments.join(" / "));
        }
        println!("      - {}", MESSAGE_HASH_SCREEN.fragments.join(" / "));
        if !case.forbidden_fragments.is_empty() {
            println!(
                "    Fragments that must not appear: {}",
                case.forbidden_fragments.join(", ")
            );
        }
        println!("    Waiting for manual approval in the web UI...");

        let signature = wait_for_sign_result(rx, case.name, true)?;
        ensure!(
            signature.iter().any(|byte| *byte != 0),
            "empty signature returned for {}",
            case.name.slug()
        );

        api.wait_for_screen_contains(HOME_SCREEN_READY, SCREEN_TIMEOUT)?;
        println!("==> Case {} passed", case.name.slug());
        return Ok(());
    }

    let mut screens = Vec::new();

    for section_index in 0..case.review_section_count {
        let section_screens = collect_section_screens(api, current.clone())?;
        current = section_screens
            .last()
            .cloned()
            .unwrap_or_else(|| current.clone());
        screens.extend(section_screens);

        if screens.len() > 80 {
            bail!(
                "review flow for {} exceeded 80 screens: {}",
                case.name.slug(),
                render_screens(&screens)
            );
        }

        if section_index + 1 < case.review_section_count {
            api.press_button("both")?;
            current = api.wait_for_screen_change(&current, SCREEN_CHANGE_TIMEOUT)?;
        }
    }

    for expected in case.expected_screens {
        ensure!(
            screens
                .iter()
                .any(|screen| screen_contains_all(screen, expected.fragments)),
            "review flow for {} missing expected screen {:?}; collected screens: {}",
            case.name.slug(),
            expected.fragments,
            render_screens(&screens)
        );
    }
    ensure!(
        screens
            .iter()
            .any(|screen| screen_contains_all(screen, MESSAGE_HASH_SCREEN.fragments)),
        "review flow for {} missing expected screen {:?}; collected screens: {}",
        case.name.slug(),
        MESSAGE_HASH_SCREEN.fragments,
        render_screens(&screens)
    );
    for forbidden in case.forbidden_fragments {
        ensure!(
            screens
                .iter()
                .all(|screen| !screen_contains(screen, forbidden)),
            "review flow for {} unexpectedly contained forbidden fragment {:?}; collected screens: {}",
            case.name.slug(),
            forbidden,
            render_screens(&screens)
        );
    }

    api.press_button("both")?;
    current = api.wait_for_screen_change(&current, SCREEN_CHANGE_TIMEOUT)?;
    ensure!(
        screen_contains_all(&current, &["Cancel", "Sign"]),
        "expected sign validator for {}; got {}",
        case.name.slug(),
        render_screen(&current)
    );
    api.press_button("right")?;
    thread::sleep(Duration::from_millis(200));
    api.press_button("both")?;
    let signature = wait_for_sign_result(rx, case.name, false)?;
    ensure!(
        signature.iter().any(|byte| *byte != 0),
        "empty signature returned for {}",
        case.name.slug()
    );

    api.wait_for_screen_contains(HOME_SCREEN_READY, SCREEN_TIMEOUT)?;
    println!("==> Case {} passed", case.name.slug());
    Ok(())
}

fn collect_section_screens(api: &SpeculosApi, start: Vec<String>) -> Result<Vec<Vec<String>>> {
    let mut screens = vec![start.clone()];
    let mut current = start;

    for _ in 0..24 {
        api.press_button("right")?;
        let Some(next) = api.try_wait_for_screen_change(&current, SCROLLER_SCROLL_TIMEOUT)? else {
            break;
        };
        screens.push(next.clone());
        current = next;
    }

    Ok(screens)
}

fn wait_for_sign_result(
    rx: mpsc::Receiver<Result<[u8; 64]>>,
    case: SmokeCaseName,
    manual_review: bool,
) -> Result<[u8; 64]> {
    if manual_review {
        rx.recv()
            .with_context(|| format!("manual review channel closed for {}", case.slug()))?
    } else {
        rx.recv_timeout(SIGN_RESULT_TIMEOUT)
            .with_context(|| format!("timed out waiting for {} sign result", case.slug()))?
    }
}

fn wait_for_load_idl_result(
    rx: mpsc::Receiver<Result<ledger_solana_cli::apdu::LoadIdlResponse>>,
    manual_review: bool,
) -> Result<ledger_solana_cli::apdu::LoadIdlResponse> {
    if manual_review {
        rx.recv().context("manual load-idl review channel closed")?
    } else {
        rx.recv_timeout(SIGN_RESULT_TIMEOUT)
            .context("timed out waiting for load-idl result")?
    }
}

fn sign_message(apdu_port: u16, derivation_path: &[u32], message: &[u8]) -> Result<[u8; 64]> {
    let mut transport =
        SpeculosTransport::connect("127.0.0.1", apdu_port).context("failed to connect to APDU")?;
    let apdus = build_sign_message_apdus(derivation_path, message)?;
    let mut signature = None;

    for (index, apdu) in apdus.iter().enumerate() {
        let response = transport.exchange(apdu)?;
        let (data, status) = decode_apdu_response(&response)?;
        assert_status(status, "sign-message")?;
        if index + 1 == apdus.len() {
            signature = Some(
                decode_sign_message_response(data)
                    .context("failed to decode sign-message response")?,
            );
        } else if !data.is_empty() {
            bail!("unexpected payload in intermediate sign-message response");
        }
    }

    signature.context("missing final sign-message response")
}

fn run_load_idl_case(
    api: &SpeculosApi,
    apdu_port: u16,
    idl_bytes: &[u8],
    attestations: &[IdlAttestation],
    manual_review: bool,
) -> Result<ledger_solana_cli::apdu::LoadIdlResponse> {
    let idl_bytes = idl_bytes.to_vec();
    let attestations = attestations.to_vec();
    let attestations_for_thread = attestations.clone();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = load_idl(
            apdu_port,
            idl_bytes.as_slice(),
            attestations_for_thread.as_slice(),
        );
        let _ = tx.send(result);
    });

    let home_screen = api.current_screen()?;
    let current = api.wait_for_screen_change(&home_screen, SCREEN_TIMEOUT)?;
    let program_id = bs58::encode(IMPORTED_SAMPLE_PROGRAM_ID.to_bytes())
        .with_alphabet(Alphabet::BITCOIN)
        .into_string();
    let signer_values = attestations
        .iter()
        .enumerate()
        .map(|(index, attestation)| {
            (
                format!("signer{}", index + 1),
                bs58::encode(attestation.signer_pubkey)
                    .with_alphabet(Alphabet::BITCOIN)
                    .into_string(),
            )
        })
        .collect::<Vec<_>>();

    if manual_review {
        println!("==> Manual review enabled for load-idl");
        println!("    Continue in the Speculos web UI: {}", api.base_url);
        println!("    Expected import review fields:");
        println!("      - programId / {}", prefix_fragment(&program_id, 4));
        for (label, signer) in &signer_values {
            println!("      - {} / {}", label, prefix_fragment(signer, 4));
        }
        println!("    Waiting for manual import approval in the web UI...");

        let response = wait_for_load_idl_result(rx, true)?;
        api.wait_for_screen_contains(HOME_SCREEN_READY, SCREEN_TIMEOUT)?;
        return Ok(response);
    }

    let (screens, validation_screen) = collect_load_idl_review_screens(api, current)?;
    assert_load_idl_review_screens(&screens, &program_id, signer_values.as_slice())?;
    ensure!(
        screen_contains(&validation_screen, "Import"),
        "expected load-idl validation screen; got {}",
        render_screen(&validation_screen)
    );

    api.press_button("both")?;
    let response = wait_for_load_idl_result(rx, false)?;
    api.wait_for_screen_contains(HOME_SCREEN_READY, SCREEN_TIMEOUT)?;
    Ok(response)
}

fn load_idl(
    apdu_port: u16,
    idl_bytes: &[u8],
    attestations: &[IdlAttestation],
) -> Result<ledger_solana_cli::apdu::LoadIdlResponse> {
    let mut transport =
        SpeculosTransport::connect("127.0.0.1", apdu_port).context("failed to connect to APDU")?;
    let apdus = build_load_idl_apdus(idl_bytes, attestations)?;
    let mut response = None;

    for (index, apdu) in apdus.iter().enumerate() {
        let raw = transport.exchange(apdu)?;
        let (data, status) = decode_apdu_response(&raw)?;
        assert_status(status, "load-idl")?;
        if index + 1 == apdus.len() {
            response =
                Some(decode_load_idl_response(data).context("failed to decode load-idl response")?);
        } else if !data.is_empty() {
            bail!("unexpected payload in intermediate load-idl response");
        }
    }

    response.context("missing final load-idl response")
}

fn collect_load_idl_review_screens(
    api: &SpeculosApi,
    start: Vec<String>,
) -> Result<(Vec<Vec<String>>, Vec<String>)> {
    let mut screens = vec![start.clone()];
    let mut current = start;

    for _ in 0..32 {
        api.press_button("right")?;
        let next = api
            .try_wait_for_screen_change(&current, SCREEN_CHANGE_TIMEOUT)?
            .context("load-idl review did not advance")?;
        screens.push(next.clone());
        current = next;

        if screen_contains(&current, "Reject") {
            api.press_button("left")?;
            let validation = api.wait_for_screen_change(&current, SCREEN_CHANGE_TIMEOUT)?;
            screens.push(validation.clone());
            return Ok((screens, validation));
        }
    }

    bail!(
        "load-idl review did not reach reject screen: {}",
        render_screens(&screens)
    )
}

fn assert_load_idl_review_screens(
    screens: &[Vec<String>],
    program_id: &str,
    signer_values: &[(String, String)],
) -> Result<()> {
    ensure!(
        screens
            .iter()
            .any(|screen| screen_contains(screen, "program")),
        "load-idl review missing programId label: {}",
        render_screens(screens)
    );
    ensure!(
        screens
            .iter()
            .any(|screen| screen_contains(screen, prefix_fragment(program_id, 4))),
        "load-idl review missing programId value: {}",
        render_screens(screens)
    );

    for (label, signer) in signer_values {
        ensure!(
            screens.iter().any(|screen| screen_contains(screen, label)),
            "load-idl review missing signer label {:?}: {}",
            label,
            render_screens(screens)
        );
        ensure!(
            screens
                .iter()
                .any(|screen| screen_contains(screen, prefix_fragment(signer, 4))),
            "load-idl review missing signer value {:?}: {}",
            label,
            render_screens(screens)
        );
    }

    Ok(())
}

fn load_idl_expect_status(
    apdu_port: u16,
    idl_bytes: &[u8],
    attestations: &[IdlAttestation],
) -> Result<u16> {
    let mut transport =
        SpeculosTransport::connect("127.0.0.1", apdu_port).context("failed to connect to APDU")?;
    let apdus = build_load_idl_apdus(idl_bytes, attestations)?;
    let mut final_status = None;

    for (index, apdu) in apdus.iter().enumerate() {
        let raw = transport.exchange(apdu)?;
        let (data, status) = decode_apdu_response(&raw)?;
        if index + 1 == apdus.len() {
            ensure!(
                data.is_empty(),
                "unexpected payload in rejected final load-idl response"
            );
            final_status = Some(status);
        } else {
            assert_status(status, "load-idl chunk")?;
            ensure!(
                data.is_empty(),
                "unexpected payload in intermediate load-idl response"
            );
        }
    }

    final_status.context("missing final load-idl status")
}

fn exchange_apdu(apdu_port: u16, apdu: &[u8]) -> Result<Vec<u8>> {
    let mut transport =
        SpeculosTransport::connect("127.0.0.1", apdu_port).context("failed to connect to APDU")?;
    transport.exchange(apdu)
}

fn assert_status(status: u16, label: &str) -> Result<()> {
    match status {
        SW_OK => Ok(()),
        SW_USER_REFUSED => bail!("{label} was refused on device"),
        other => bail!("{label} returned unexpected status 0x{other:04x}"),
    }
}

fn build_case(case: SmokeCaseName, signer_pubkey: [u8; 32]) -> Result<SmokeCase> {
    let expected_screens = match case {
        SmokeCaseName::SystemTransfer => SYSTEM_TRANSFER_SCREENS,
        SmokeCaseName::ComputeBudgetLimit => COMPUTE_BUDGET_LIMIT_SCREENS,
        SmokeCaseName::AtaCreate => ATA_CREATE_SCREENS,
        SmokeCaseName::TokenTransfer => TOKEN_TRANSFER_SCREENS,
        SmokeCaseName::ImportedProgramGeneric | SmokeCaseName::ImportedProgramDecoded => {
            unreachable!("imported program cases are built separately")
        }
    };

    let message = match case {
        SmokeCaseName::SystemTransfer => build_system_transfer_message(signer_pubkey)?,
        SmokeCaseName::ComputeBudgetLimit => build_compute_budget_limit_message(signer_pubkey)?,
        SmokeCaseName::AtaCreate => build_ata_create_message(signer_pubkey)?,
        SmokeCaseName::TokenTransfer => build_token_transfer_message(signer_pubkey)?,
        SmokeCaseName::ImportedProgramGeneric | SmokeCaseName::ImportedProgramDecoded => {
            unreachable!("imported program cases are built separately")
        }
    };
    let message_hash = message_sha256(&message);

    Ok(SmokeCase {
        name: case,
        message,
        message_hash,
        review_section_count: 2,
        expected_screens,
        forbidden_fragments: NO_FORBIDDEN_FRAGMENTS,
    })
}

fn build_imported_sample_case(signer_pubkey: [u8; 32], decoded: bool) -> Result<SmokeCase> {
    let message = build_imported_sample_message(signer_pubkey)?;
    let message_hash = message_sha256(&message);

    Ok(SmokeCase {
        name: if decoded {
            SmokeCaseName::ImportedProgramDecoded
        } else {
            SmokeCaseName::ImportedProgramGeneric
        },
        message,
        message_hash,
        review_section_count: 2,
        expected_screens: if decoded {
            IMPORTED_PROGRAM_SCREENS
        } else {
            UNKNOWN_PROGRAM_GENERIC_SCREENS
        },
        forbidden_fragments: if decoded {
            NO_FORBIDDEN_FRAGMENTS
        } else {
            UNKNOWN_PROGRAM_GENERIC_FORBIDDEN
        },
    })
}

fn build_system_transfer_message(signer_pubkey: [u8; 32]) -> Result<Vec<u8>> {
    let payer = Address::from(signer_pubkey);
    let destination = repeated_address(0x22);
    let instruction = system_instruction::transfer(&payer, &destination, 42_000_000_000);
    Ok(build_legacy_message(&payer, &[instruction]))
}

fn build_compute_budget_limit_message(signer_pubkey: [u8; 32]) -> Result<Vec<u8>> {
    let payer = Address::from(signer_pubkey);
    let instruction = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    Ok(build_legacy_message(&payer, &[instruction]))
}

fn build_ata_create_message(signer_pubkey: [u8; 32]) -> Result<Vec<u8>> {
    let payer = Address::from(signer_pubkey);
    let wallet = repeated_address(0x68);
    let mint = repeated_address(0x69);
    let instruction = associated_token_instruction::create_associated_token_account(
        &payer,
        &wallet,
        &mint,
        &spl_token_interface::id(),
    );
    Ok(build_legacy_message(&payer, &[instruction]))
}

fn build_token_transfer_message(signer_pubkey: [u8; 32]) -> Result<Vec<u8>> {
    let authority = Address::from(signer_pubkey);
    let source = repeated_address(0x89);
    let destination = repeated_address(0x8a);
    let instruction = token_instruction::transfer(
        &spl_token_interface::id(),
        &source,
        &destination,
        &authority,
        &[],
        42_000_000,
    )
    .context("failed to build token transfer test instruction")?;
    Ok(build_legacy_message(&authority, &[instruction]))
}

fn build_imported_sample_message(signer_pubkey: [u8; 32]) -> Result<Vec<u8>> {
    let payer = Address::from(signer_pubkey);
    let instruction = Instruction {
        program_id: IMPORTED_SAMPLE_PROGRAM_ID,
        accounts: vec![],
        data: imported_sample_instruction_data(),
    };
    Ok(build_legacy_message(&payer, &[instruction]))
}

fn build_legacy_message(payer: &Address, instructions: &[Instruction]) -> Vec<u8> {
    Message::new(instructions, Some(payer)).serialize()
}

fn message_sha256(message: &[u8]) -> String {
    let digest = Sha256::digest(message);
    bs58::encode(digest)
        .with_alphabet(Alphabet::BITCOIN)
        .into_string()
}

fn imported_sample_instruction_data() -> Vec<u8> {
    let mut out = Vec::with_capacity(17);
    out.extend_from_slice(&hex::decode("2122232425262728").expect("valid sample selector"));
    out.extend_from_slice(&42u64.to_le_bytes());
    out.push(1);
    out
}

fn imported_sample_attestation() -> Result<IdlAttestation> {
    Ok(IdlAttestation {
        signer_pubkey: decode_hex_array::<32>(IMPORTED_SAMPLE_SIGNER_PUBKEY_HEX)?,
        signature: decode_hex_array::<64>(IMPORTED_SAMPLE_SIGNATURE_HEX)?,
    })
}

fn repeated_address(byte: u8) -> Address {
    Address::from([byte; 32])
}

fn decode_hex_array<const N: usize>(value: &str) -> Result<[u8; N]> {
    let bytes = hex::decode(value).with_context(|| format!("invalid hex fixture: {value}"))?;
    if bytes.len() != N {
        bail!("expected {N} decoded hex bytes, got {}", bytes.len());
    }

    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn selected_cases(args: &SpeculosSmokeArgs) -> Vec<SmokeCaseName> {
    if args.cases.is_empty() {
        SmokeCaseName::all().to_vec()
    } else {
        args.cases.clone()
    }
}

fn resolve_ports(api_port: Option<u16>, apdu_port: Option<u16>, vnc_port: Option<u16>) -> Ports {
    let api = api_port.unwrap_or(DEFAULT_SPECULOS_API_PORT);
    let apdu = apdu_port.unwrap_or(DEFAULT_SPECULOS_APDU_PORT);
    let vnc = vnc_port.unwrap_or(DEFAULT_SPECULOS_VNC_PORT);
    Ports { api, apdu, vnc }
}

fn repo_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("failed to resolve repo root"))
}

fn run_checked(command: &mut Command, label: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("failed to run {label}"))?;
    ensure!(status.success(), "{label} exited with {status}");
    Ok(())
}

fn screen_contains(screen: &[String], fragment: &str) -> bool {
    screen.iter().any(|line| fragment_matches(line, fragment))
}

fn screen_contains_all(screen: &[String], fragments: &[&str]) -> bool {
    let joined = screen.join("\n");
    fragments
        .iter()
        .all(|fragment| fragment_matches(joined.as_str(), fragment))
}

fn fragment_matches(haystack: &str, fragment: &str) -> bool {
    let haystack_variants = normalized_variants(haystack);
    let fragment_variants = normalized_variants(fragment);

    haystack_variants.iter().any(|haystack_variant| {
        fragment_variants
            .iter()
            .filter(|fragment_variant| !fragment_variant.is_empty())
            .any(|fragment_variant| haystack_variant.contains(fragment_variant))
    })
}

fn normalized_variants(value: &str) -> [String; 4] {
    let lower = value.to_ascii_lowercase();
    let stripped = strip_ascii_whitespace(lower.as_str());
    let lower_without_first = lower
        .char_indices()
        .nth(1)
        .map(|(index, _)| lower[index..].to_string())
        .unwrap_or_default();
    let stripped_without_first = strip_ascii_whitespace(lower_without_first.as_str());

    [lower, stripped, lower_without_first, stripped_without_first]
}

fn strip_ascii_whitespace(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect()
}

fn prefix_fragment(value: &str, max_chars: usize) -> &str {
    if value.len() <= max_chars {
        value
    } else {
        &value[..max_chars]
    }
}

fn render_screen(screen: &[String]) -> String {
    if screen.is_empty() {
        "(empty screen)".to_string()
    } else {
        screen.join(" | ")
    }
}

fn render_screens(screens: &[Vec<String>]) -> String {
    screens
        .iter()
        .map(|screen| render_screen(screen))
        .collect::<Vec<_>>()
        .join(" || ")
}
