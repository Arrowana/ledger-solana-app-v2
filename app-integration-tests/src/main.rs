use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure, Context, Result};
use bs58::Alphabet;
use clap::{Args, Parser, Subcommand, ValueEnum};
use ledger_solana_cli::apdu::{
    build_get_app_config_apdu, build_get_pubkey_apdu, build_sign_message_apdus,
    decode_apdu_response, decode_get_app_config_response, decode_get_pubkey_response,
    decode_sign_message_response,
};
use ledger_solana_cli::constants::{SW_OK, SW_USER_REFUSED};
use ledger_solana_cli::derivation::parse_derivation_path;
use ledger_solana_cli::transport::{DeviceTransport, SpeculosTransport};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use solana_instruction::{AccountMeta, Instruction};
use solana_message::Message;
use solana_pubkey::Pubkey;

const DEFAULT_DERIVATION_PATH: &str = "m/44'/501'/0'/0'";
const SCREEN_TIMEOUT: Duration = Duration::from_secs(20);
const SCREEN_CHANGE_TIMEOUT: Duration = Duration::from_secs(5);
const SIGN_RESULT_TIMEOUT: Duration = Duration::from_secs(30);
const HOME_SCREEN_TITLE: &str = "Solana v2";
const HOME_SCREEN_READY: &str = "app is ready";
const REVIEW_TITLE: &str = "Review Solana tx";
const REVIEW_APPROVE: &str = "Sign transaction";
const SYSTEM_PROGRAM_ID: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");
const COMPUTE_BUDGET_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("ComputeBudget111111111111111111111111111111");
const ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const TOKEN_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

#[derive(Parser)]
#[command(name = "app-integration-tests")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    SpeculosSmoke(SpeculosSmokeArgs),
}

#[derive(Args, Clone)]
struct SpeculosSmokeArgs {
    #[arg(long, default_value_t = false)]
    skip_build: bool,
    #[arg(long, default_value_t = false)]
    manual_review: bool,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SmokeCaseName {
    SystemTransfer,
    ComputeBudgetLimit,
    AtaCreate,
    TokenTransfer,
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
    expected_screens: &'static [ExpectedScreen],
}

const MESSAGE_HASH_SCREEN: ExpectedScreen = ExpectedScreen {
    fragments: &["Message SHA-256"],
};

const SYSTEM_TRANSFER_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["Instruction", "system", "transferSol"],
    },
    ExpectedScreen {
        fragments: &["amount", "42"],
    },
    ExpectedScreen {
        fragments: &["source"],
    },
];

const COMPUTE_BUDGET_LIMIT_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["Instruction", "compute", "budget", "setComputeUnitLimit"],
    },
    ExpectedScreen {
        fragments: &["units", "1400000"],
    },
];

const ATA_CREATE_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["Instruction", "associated", "token", "create"],
    },
    ExpectedScreen {
        fragments: &["Arguments", "none"],
    },
    ExpectedScreen {
        fragments: &["funder"],
    },
];

const TOKEN_TRANSFER_SCREENS: &[ExpectedScreen] = &[
    ExpectedScreen {
        fragments: &["Instruction", "token", "transfer"],
    },
    ExpectedScreen {
        fragments: &["amount", "42"],
    },
    ExpectedScreen {
        fragments: &["source"],
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
    }
}

fn run_speculos_smoke(args: SpeculosSmokeArgs) -> Result<()> {
    let root = repo_root()?;
    let ports = resolve_ports(&args);
    let derivation_path = parse_derivation_path(&args.derivation_path)?;
    let cases = selected_cases(&args);

    if !args.skip_build {
        println!("==> Building Ledger app with scripts/build-ledger.sh");
        run_checked(
            Command::new("bash")
                .arg("./scripts/build-ledger.sh")
                .current_dir(&root),
            "scripts/build-ledger.sh",
        )?;
    }

    println!(
        "==> Launching Speculos on api={}, apdu={}, vnc={}",
        ports.api, ports.apdu, ports.vnc
    );
    let _speculos = SpeculosProcess::spawn(&root, &ports)?;
    let api = SpeculosApi::new(ports.api)?;
    api.wait_for_screen_contains(HOME_SCREEN_TITLE, SCREEN_TIMEOUT)?;
    api.wait_for_screen_contains(HOME_SCREEN_READY, SCREEN_TIMEOUT)?;
    println!("==> Speculos is ready");

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
        let smoke_case = build_case(case)?;
        run_sign_case(
            &api,
            ports.apdu,
            derivation_path.as_slice(),
            &smoke_case,
            args.manual_review,
        )?;
    }

    println!("==> Speculos smoke tests completed successfully");
    Ok(())
}

fn read_app_config(apdu_port: u16) -> Result<ledger_solana_cli::apdu::AppConfigResponse> {
    let response = exchange_apdu(apdu_port, &build_get_app_config_apdu()?)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "app-config")?;
    decode_get_app_config_response(data).context("failed to decode app-config response")
}

fn read_pubkey(apdu_port: u16, derivation_path: &[u32]) -> Result<[u8; 32]> {
    let response = exchange_apdu(apdu_port, &build_get_pubkey_apdu(derivation_path, false)?)?;
    let (data, status) = decode_apdu_response(&response)?;
    assert_status(status, "get-pubkey")?;
    decode_get_pubkey_response(data).context("failed to decode get-pubkey response")
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

    let mut current = api.wait_for_screen_contains(REVIEW_TITLE, SCREEN_TIMEOUT)?;

    if manual_review {
        println!("==> Manual review enabled for {}", case.name.slug());
        println!("    Continue in the Speculos web UI: {}", api.base_url);
        println!("    Message SHA-256: {}", case.message_hash);
        println!("    Expected decoded screens:");
        for expected in case.expected_screens {
            println!("      - {}", expected.fragments.join(" / "));
        }
        println!("      - {}", MESSAGE_HASH_SCREEN.fragments.join(" / "));
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

    let mut screens = vec![current.clone()];

    while !screen_contains(&current, REVIEW_APPROVE) {
        api.press_button("right")?;
        current = api.wait_for_screen_change(&current, SCREEN_CHANGE_TIMEOUT)?;
        screens.push(current.clone());
        if screens.len() > 40 {
            bail!(
                "review flow for {} exceeded 40 screens: {}",
                case.name.slug(),
                render_screens(&screens)
            );
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

fn build_case(case: SmokeCaseName) -> Result<SmokeCase> {
    let expected_screens = match case {
        SmokeCaseName::SystemTransfer => SYSTEM_TRANSFER_SCREENS,
        SmokeCaseName::ComputeBudgetLimit => COMPUTE_BUDGET_LIMIT_SCREENS,
        SmokeCaseName::AtaCreate => ATA_CREATE_SCREENS,
        SmokeCaseName::TokenTransfer => TOKEN_TRANSFER_SCREENS,
    };

    let message = match case {
        SmokeCaseName::SystemTransfer => build_system_transfer_message()?,
        SmokeCaseName::ComputeBudgetLimit => build_compute_budget_limit_message()?,
        SmokeCaseName::AtaCreate => build_ata_create_message()?,
        SmokeCaseName::TokenTransfer => build_token_transfer_message()?,
    };
    let message_hash = message_sha256(&message);

    Ok(SmokeCase {
        name: case,
        message,
        message_hash,
        expected_screens,
    })
}

fn build_system_transfer_message() -> Result<Vec<u8>> {
    let payer = repeated_pubkey(0x11);
    let destination = repeated_pubkey(0x22);
    let instruction = Instruction {
        program_id: SYSTEM_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(destination, false),
        ],
        data: system_transfer_data(42),
    };
    Ok(build_legacy_message(&payer, &[instruction]))
}

fn build_compute_budget_limit_message() -> Result<Vec<u8>> {
    let payer = repeated_pubkey(0x44);
    let instruction = Instruction {
        program_id: COMPUTE_BUDGET_PROGRAM_ID,
        accounts: vec![],
        data: compute_budget_limit_data(1_400_000),
    };
    Ok(build_legacy_message(&payer, &[instruction]))
}

fn build_ata_create_message() -> Result<Vec<u8>> {
    let payer = repeated_pubkey(0x66);
    let associated_account = repeated_pubkey(0x67);
    let wallet = repeated_pubkey(0x68);
    let mint = repeated_pubkey(0x69);
    let instruction = Instruction {
        program_id: ASSOCIATED_TOKEN_ACCOUNT_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new(associated_account, false),
            AccountMeta::new_readonly(wallet, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
        ],
        data: vec![0],
    };
    Ok(build_legacy_message(&payer, &[instruction]))
}

fn build_token_transfer_message() -> Result<Vec<u8>> {
    let authority = repeated_pubkey(0x88);
    let source = repeated_pubkey(0x89);
    let destination = repeated_pubkey(0x8a);
    let instruction = Instruction {
        program_id: TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(source, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(authority, true),
        ],
        data: token_transfer_data(42),
    };
    Ok(build_legacy_message(&authority, &[instruction]))
}

fn build_legacy_message(payer: &Pubkey, instructions: &[Instruction]) -> Vec<u8> {
    Message::new(instructions, Some(payer)).serialize()
}

fn message_sha256(message: &[u8]) -> String {
    let digest = Sha256::digest(message);
    bs58::encode(digest)
        .with_alphabet(Alphabet::BITCOIN)
        .into_string()
}

fn system_transfer_data(amount: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&amount.to_le_bytes());
    out
}

fn compute_budget_limit_data(units: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    out.push(2);
    out.extend_from_slice(&units.to_le_bytes());
    out
}

fn token_transfer_data(amount: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.push(3);
    out.extend_from_slice(&amount.to_le_bytes());
    out
}

fn repeated_pubkey(byte: u8) -> Pubkey {
    Pubkey::new_from_array([byte; 32])
}

fn selected_cases(args: &SpeculosSmokeArgs) -> Vec<SmokeCaseName> {
    if args.cases.is_empty() {
        SmokeCaseName::all().to_vec()
    } else {
        args.cases.clone()
    }
}

fn resolve_ports(args: &SpeculosSmokeArgs) -> Ports {
    let api = args.api_port.unwrap_or_else(find_free_port);
    let apdu = args
        .apdu_port
        .unwrap_or_else(|| find_free_port_excluding(&[api]));
    let vnc = args
        .vnc_port
        .unwrap_or_else(|| find_free_port_excluding(&[api, apdu]));
    Ports { api, apdu, vnc }
}

fn find_free_port() -> u16 {
    find_free_port_excluding(&[])
}

fn find_free_port_excluding(excluded: &[u16]) -> u16 {
    loop {
        let port = TcpListener::bind(("127.0.0.1", 0))
            .expect("failed to allocate free port")
            .local_addr()
            .expect("failed to read free port")
            .port();
        if !excluded.contains(&port) {
            return port;
        }
    }
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
    screen.iter().any(|line| line.contains(fragment))
}

fn screen_contains_all(screen: &[String], fragments: &[&str]) -> bool {
    let joined = screen.join("\n");
    fragments.iter().all(|fragment| joined.contains(fragment))
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
