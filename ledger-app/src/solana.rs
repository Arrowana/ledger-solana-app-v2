use alloc::vec::Vec;

use crate::AppSW;
use ledger_device_sdk::ecc::Ed25519;

pub const APP_CLA: u8 = 0xe0;
pub const INS_GET_APP_CONFIG: u8 = 0x04;
pub const INS_GET_PUBKEY: u8 = 0x05;
pub const INS_SIGN_MESSAGE: u8 = 0x06;

pub const P1_NON_CONFIRM: u8 = 0x00;
pub const P1_CONFIRM: u8 = 0x01;

pub const P2_EXTEND: u8 = 0x01;
pub const P2_MORE: u8 = 0x02;

pub const PUBKEY_LENGTH: usize = 32;
pub const SIGNATURE_LENGTH: usize = 64;
pub const MAX_BIP32_PATH_LENGTH: usize = 10;
pub const MAX_SIGN_PAYLOAD_LENGTH: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivationPath {
    components: [u32; MAX_BIP32_PATH_LENGTH],
    length: u8,
}

impl DerivationPath {
    pub fn as_slice(&self) -> &[u32] {
        &self.components[..self.length as usize]
    }
}

pub struct ParsedSignPayload<'a> {
    pub path: DerivationPath,
    pub message: &'a [u8],
}

pub struct SignMessageContext {
    payload: Vec<u8>,
    in_progress: bool,
}

impl SignMessageContext {
    pub fn new() -> Self {
        Self {
            payload: Vec::new(),
            in_progress: false,
        }
    }

    pub fn reset(&mut self) {
        self.payload.clear();
        self.in_progress = false;
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn ingest(&mut self, p2: u8, data: &[u8]) -> Result<bool, AppSW> {
        let has_extend = (p2 & P2_EXTEND) != 0;
        let has_more = (p2 & P2_MORE) != 0;
        if p2 & !(P2_EXTEND | P2_MORE) != 0 {
            self.reset();
            return Err(AppSW::WrongP1P2);
        }

        if !self.in_progress {
            if has_extend {
                self.reset();
                return Err(AppSW::WrongP1P2);
            }
            self.payload.clear();
            self.in_progress = true;
        } else if !has_extend {
            self.reset();
            return Err(AppSW::WrongP1P2);
        }

        if self.payload.len() + data.len() > MAX_SIGN_PAYLOAD_LENGTH {
            self.reset();
            return Err(AppSW::WrongApduLength);
        }

        self.payload.extend_from_slice(data);
        if has_more {
            return Ok(false);
        }

        self.in_progress = false;
        Ok(true)
    }
}

pub fn app_config_response() -> Result<[u8; 5], AppSW> {
    let (major, minor, patch) = parse_version_string(env!("CARGO_PKG_VERSION"))?;
    Ok([
        0, // blind signing disabled
        0, // long public key display mode
        major, minor, patch,
    ])
}

pub fn parse_derivation_path(data: &[u8]) -> Result<(DerivationPath, usize), AppSW> {
    if data.is_empty() {
        return Err(AppSW::WrongApduLength);
    }

    let path_len = data[0] as usize;
    if path_len < 2 || path_len > MAX_BIP32_PATH_LENGTH {
        return Err(AppSW::WrongApduLength);
    }

    let encoded_len = 1 + path_len * 4;
    if data.len() < encoded_len {
        return Err(AppSW::WrongApduLength);
    }

    let mut components = [0u32; MAX_BIP32_PATH_LENGTH];
    for (index, chunk) in data[1..encoded_len].chunks(4).enumerate() {
        components[index] =
            u32::from_be_bytes(chunk.try_into().map_err(|_| AppSW::WrongApduLength)?);
    }

    if components[0] != 0x8000_002c || components[1] != 0x8000_01f5 {
        return Err(AppSW::WrongApduLength);
    }

    Ok((
        DerivationPath {
            components,
            length: path_len as u8,
        },
        encoded_len,
    ))
}

pub fn parse_sign_payload(payload: &[u8]) -> Result<ParsedSignPayload<'_>, AppSW> {
    if payload.is_empty() || payload[0] != 1 {
        return Err(AppSW::InvalidData);
    }

    let (path, consumed) = parse_derivation_path(&payload[1..])?;
    let message = &payload[1 + consumed..];
    if message.is_empty() {
        return Err(AppSW::InvalidData);
    }

    Ok(ParsedSignPayload { path, message })
}

pub fn derive_pubkey(path: &[u32]) -> Result<[u8; PUBKEY_LENGTH], AppSW> {
    let sk = Ed25519::derive_from_path_slip10(path);
    let pk = sk.public_key().map_err(|_| AppSW::KeyDeriveFail)?;
    let mut out = [0u8; PUBKEY_LENGTH];
    out.copy_from_slice(&pk.pubkey[1..33]);
    Ok(out)
}

pub fn sign_message(path: &[u32], message: &[u8]) -> Result<[u8; SIGNATURE_LENGTH], AppSW> {
    let sk = Ed25519::derive_from_path_slip10(path);
    let (signature, signature_length) = sk.sign(message).map_err(|_| AppSW::KeyDeriveFail)?;
    if signature_length as usize != SIGNATURE_LENGTH {
        return Err(AppSW::ConditionsNotSatisfied);
    }
    Ok(signature)
}

fn parse_version_string(input: &str) -> Result<(u8, u8, u8), AppSW> {
    let mut parts = input.split('.');
    let major = parts
        .next()
        .and_then(|part| part.parse::<u8>().ok())
        .ok_or(AppSW::InvalidData)?;
    let minor = parts
        .next()
        .and_then(|part| part.parse::<u8>().ok())
        .ok_or(AppSW::InvalidData)?;
    let patch = parts
        .next()
        .and_then(|part| part.parse::<u8>().ok())
        .ok_or(AppSW::InvalidData)?;
    Ok((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_path_bytes() -> [u8; 13] {
        let mut out = [0u8; 13];
        out[0] = 3;
        out[1..5].copy_from_slice(&0x8000_002c_u32.to_be_bytes());
        out[5..9].copy_from_slice(&0x8000_01f5_u32.to_be_bytes());
        out[9..13].copy_from_slice(&0x8000_0000_u32.to_be_bytes());
        out
    }

    #[test]
    fn parses_sign_payload_with_official_prefix() {
        let path = sample_path_bytes();
        let mut payload = Vec::new();
        payload.push(1);
        payload.extend_from_slice(&path);
        payload.extend_from_slice(&[0xaa, 0xbb, 0xcc]);

        let parsed = parse_sign_payload(&payload).unwrap();
        assert_eq!(
            parsed.path.as_slice(),
            &[0x8000_002c, 0x8000_01f5, 0x8000_0000]
        );
        assert_eq!(parsed.message, &[0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn reassembles_chunked_payload_with_official_p2_flags() {
        let path = sample_path_bytes();
        let mut payload = Vec::new();
        payload.push(1);
        payload.extend_from_slice(&path);
        payload.extend_from_slice(&[1, 2, 3, 4, 5]);

        let mut context = SignMessageContext::new();
        assert!(!context.ingest(P2_MORE, &payload[..8]).unwrap());
        assert!(context.ingest(P2_EXTEND, &payload[8..]).unwrap());
        assert_eq!(context.payload(), payload.as_slice());
    }
}
