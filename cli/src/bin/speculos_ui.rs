use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;

#[derive(Parser)]
#[command(name = "speculos-ui")]
struct Cli {
    #[command(subcommand)]
    command: Command,
    #[arg(long, env = "SPECULOS_API_PORT", default_value_t = 5001)]
    api_port: u16,
    #[arg(long, env = "SPECULOS_AUTOMATION_PORT", default_value_t = 41000)]
    automation_port: u16,
    #[arg(long, env = "SPECULOS_BUTTON_PORT", default_value_t = 42000)]
    button_port: u16,
    #[arg(long, env = "SPECULOS_API_URL")]
    api_url: Option<String>,
    #[arg(long, env = "SPECULOS_AUTOMATION_HOST", default_value = "127.0.0.1")]
    automation_host: String,
    #[arg(long, env = "SPECULOS_BUTTON_HOST", default_value = "127.0.0.1")]
    button_host: String,
}

#[derive(Subcommand)]
enum Command {
    Screen,
    Events,
    Left,
    Right,
    Both,
    ClearEvents,
}

#[derive(Debug, Deserialize)]
struct ScreenEvent {
    text: Option<String>,
    clear: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ScreenResponse {
    events: Option<Vec<ScreenEvent>>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let api_base = cli
        .api_url
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", cli.api_port));

    match cli.command {
        Command::Screen => {
            print_current_screen(&api_base, &cli.automation_host, cli.automation_port)
        }
        Command::Events => stream_events(&api_base, &cli.automation_host, cli.automation_port),
        Command::Left => press_button(
            "left",
            &api_base,
            &cli.button_host,
            cli.button_port,
            &cli.automation_host,
            cli.automation_port,
        ),
        Command::Right => press_button(
            "right",
            &api_base,
            &cli.button_host,
            cli.button_port,
            &cli.automation_host,
            cli.automation_port,
        ),
        Command::Both => press_button(
            "both",
            &api_base,
            &cli.button_host,
            cli.button_port,
            &cli.automation_host,
            cli.automation_port,
        ),
        Command::ClearEvents => clear_events(&api_base),
    }
}

fn print_current_screen(api_base: &str, automation_host: &str, automation_port: u16) -> Result<()> {
    match fetch_current_screen(api_base) {
        Ok(lines) if !lines.is_empty() => {
            println!("API: {api_base}");
            for line in lines {
                println!("{line}");
            }
            Ok(())
        }
        _ => {
            let lines = collect_automation_lines(
                automation_host,
                automation_port,
                Duration::from_millis(250),
            )?;
            println!("Automation: {automation_host}:{automation_port}");
            if lines.is_empty() {
                println!("(no text events)");
            } else {
                for line in lines {
                    println!("{line}");
                }
            }
            Ok(())
        }
    }
}

fn stream_events(api_base: &str, automation_host: &str, automation_port: u16) -> Result<()> {
    let client = Client::new();
    let response = client.get(format!("{api_base}/events?stream=true")).send();
    match response {
        Ok(mut response) => {
            let mut body = String::new();
            response.read_to_string(&mut body)?;
            for line in body.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    let event: ScreenEvent = serde_json::from_str(data)?;
                    if event.clear.unwrap_or(false) {
                        println!("--- clear ---");
                    } else if let Some(text) = event.text {
                        println!("{text}");
                    }
                }
            }
            Ok(())
        }
        Err(_) => {
            let socket = TcpStream::connect((automation_host, automation_port))
                .with_context(|| format!("failed to connect to automation socket {automation_host}:{automation_port}"))?;
            let mut reader = BufReader::new(socket);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line)?;
                if line.trim().is_empty() {
                    continue;
                }
                let event: ScreenEvent = serde_json::from_str(line.trim())?;
                if event.clear.unwrap_or(false) {
                    println!("--- clear ---");
                } else if let Some(text) = event.text {
                    println!("{text}");
                }
            }
        }
    }
}

fn press_button(
    button: &str,
    api_base: &str,
    button_host: &str,
    button_port: u16,
    automation_host: &str,
    automation_port: u16,
) -> Result<()> {
    let client = Client::new();
    if client
        .post(format!("{api_base}/button/{button}"))
        .json(&json!({ "action": "press-and-release" }))
        .send()
        .is_err()
    {
        let mut socket = TcpStream::connect((button_host, button_port)).with_context(|| {
            format!("failed to connect to button socket {button_host}:{button_port}")
        })?;
        socket.write_all(button.as_bytes())?;
        socket.write_all(b"\n")?;
    }

    thread::sleep(Duration::from_millis(150));
    print_current_screen(api_base, automation_host, automation_port)
}

fn clear_events(api_base: &str) -> Result<()> {
    Client::new()
        .delete(format!("{api_base}/events"))
        .send()?
        .error_for_status()?;
    Ok(())
}

fn fetch_current_screen(api_base: &str) -> Result<Vec<String>> {
    let response = Client::new()
        .get(format!("{api_base}/events?currentscreenonly=true"))
        .send()?
        .error_for_status()?
        .json::<ScreenResponse>()?;

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

fn collect_automation_lines(host: &str, port: u16, duration: Duration) -> Result<Vec<String>> {
    let socket = TcpStream::connect((host, port))
        .with_context(|| format!("failed to connect to automation socket {host}:{port}"))?;
    socket.set_nonblocking(true)?;
    let start = Instant::now();
    let mut reader = BufReader::new(socket);
    let mut lines = Vec::new();

    while start.elapsed() < duration {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => thread::sleep(Duration::from_millis(10)),
            Ok(_) => {
                let event: ScreenEvent = serde_json::from_str(line.trim())?;
                if let Some(text) = event.text {
                    if !lines.iter().any(|entry| entry == &text) {
                        lines.push(text);
                    }
                }
            }
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }

    Ok(lines)
}
