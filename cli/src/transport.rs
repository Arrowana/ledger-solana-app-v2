use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use hidapi::{DeviceInfo, HidApi, HidDevice};

use crate::constants::{HID_CHANNEL, HID_TAG_APDU, LEDGER_VENDOR_ID, TransportKind};

pub trait DeviceTransport {
    fn exchange(&mut self, apdu: &[u8]) -> Result<Vec<u8>>;
}

pub enum Transport {
    Speculos(SpeculosTransport),
    Hid(LedgerHidTransport),
}

impl DeviceTransport for Transport {
    fn exchange(&mut self, apdu: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Speculos(transport) => transport.exchange(apdu),
            Self::Hid(transport) => transport.exchange(apdu),
        }
    }
}

pub fn open_transport(
    kind: TransportKind,
    speculos_host: &str,
    speculos_port: u16,
) -> Result<Transport> {
    match kind {
        TransportKind::Speculos => Ok(Transport::Speculos(SpeculosTransport::connect(
            speculos_host,
            speculos_port,
        )?)),
        TransportKind::Hid => Ok(Transport::Hid(LedgerHidTransport::open()?)),
    }
}

pub struct SpeculosTransport {
    socket: TcpStream,
}

impl SpeculosTransport {
    pub fn connect(host: &str, port: u16) -> Result<Self> {
        let socket = TcpStream::connect((host, port))
            .with_context(|| format!("failed to connect to Speculos at {host}:{port}"))?;
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .context("failed to configure Speculos read timeout")?;
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .context("failed to configure Speculos write timeout")?;
        Ok(Self { socket })
    }
}

impl DeviceTransport for SpeculosTransport {
    fn exchange(&mut self, apdu: &[u8]) -> Result<Vec<u8>> {
        let length = (apdu.len() as u32).to_be_bytes();
        self.socket.write_all(&length)?;
        self.socket.write_all(apdu)?;

        let mut response_len = [0u8; 4];
        self.socket.read_exact(&mut response_len)?;
        let payload_len = u32::from_be_bytes(response_len) as usize;

        let mut payload = vec![0u8; payload_len];
        self.socket.read_exact(&mut payload)?;

        let mut status = [0u8; 2];
        self.socket.read_exact(&mut status)?;
        payload.extend_from_slice(&status);
        Ok(payload)
    }
}

pub struct LedgerHidTransport {
    device: HidDevice,
}

impl LedgerHidTransport {
    pub fn open() -> Result<Self> {
        let api = HidApi::new().context("failed to initialize hidapi")?;
        let info = api
            .device_list()
            .find(|device| is_ledger_candidate(device))
            .cloned()
            .context("no Ledger HID device found")?;
        let device = info
            .open_device(&api)
            .context("failed to open Ledger HID device")?;
        Ok(Self { device })
    }
}

impl DeviceTransport for LedgerHidTransport {
    fn exchange(&mut self, apdu: &[u8]) -> Result<Vec<u8>> {
        for frame in wrap_apdu(apdu) {
            let written = self.device.write(&frame)?;
            if written != frame.len() {
                bail!("short HID write: wrote {written} of {}", frame.len());
            }
        }
        unwrap_response(&self.device)
    }
}

fn is_ledger_candidate(device: &DeviceInfo) -> bool {
    device.vendor_id() == LEDGER_VENDOR_ID
}

fn wrap_apdu(apdu: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut sequence: u16 = 0;
    let mut offset = 0usize;

    while offset < apdu.len() || sequence == 0 {
        let mut packet = [0u8; 65];
        let data = &mut packet[1..];
        data[0..2].copy_from_slice(&HID_CHANNEL.to_be_bytes());
        data[2] = HID_TAG_APDU;
        data[3..5].copy_from_slice(&sequence.to_be_bytes());

        let capacity = if sequence == 0 {
            data[5..7].copy_from_slice(&(apdu.len() as u16).to_be_bytes());
            57usize
        } else {
            59usize
        };

        let start = if sequence == 0 { 7 } else { 5 };
        let remaining = apdu.len().saturating_sub(offset);
        let chunk = remaining.min(capacity);
        if chunk > 0 {
            data[start..start + chunk].copy_from_slice(&apdu[offset..offset + chunk]);
            offset += chunk;
        }

        frames.push(packet.to_vec());
        sequence = sequence.wrapping_add(1);
        if chunk == 0 {
            break;
        }
    }

    frames
}

fn unwrap_response(device: &HidDevice) -> Result<Vec<u8>> {
    let mut buffer = [0u8; 65];
    let first_len = device.read_timeout(&mut buffer, 5000)?;
    if first_len == 0 {
        bail!("timeout waiting for Ledger HID response");
    }
    let first = normalize_hid_packet(&buffer[..first_len])?;
    validate_frame_header(first, 0)?;
    if first.len() < 7 {
        bail!("short first HID frame");
    }

    let total_len = u16::from_be_bytes([first[5], first[6]]) as usize;
    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(&first[7..]);

    let mut sequence: u16 = 1;
    while out.len() < total_len {
        let len = device.read_timeout(&mut buffer, 5000)?;
        if len == 0 {
            bail!("timeout waiting for continuation HID frame");
        }
        let packet = normalize_hid_packet(&buffer[..len])?;
        validate_frame_header(packet, sequence)?;
        out.extend_from_slice(&packet[5..]);
        sequence = sequence.wrapping_add(1);
    }

    out.truncate(total_len);
    Ok(out)
}

fn normalize_hid_packet(packet: &[u8]) -> Result<&[u8]> {
    match packet.len() {
        64 => Ok(packet),
        65 if packet[0] == 0 => Ok(&packet[1..]),
        len => Err(anyhow!("unexpected HID packet length: {len}")),
    }
}

fn validate_frame_header(packet: &[u8], expected_sequence: u16) -> Result<()> {
    if packet.len() < 5 {
        bail!("short HID packet");
    }
    let channel = u16::from_be_bytes([packet[0], packet[1]]);
    let tag = packet[2];
    let sequence = u16::from_be_bytes([packet[3], packet[4]]);
    if channel != HID_CHANNEL || tag != HID_TAG_APDU || sequence != expected_sequence {
        bail!("unexpected HID frame header");
    }
    Ok(())
}

