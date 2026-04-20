use anyhow::{bail, Result};

use crate::constants::AppInstruction;
use crate::derivation::serialize_derivation_path;

const SOLANA_PAYLOAD_VERSION: u8 = 1;
const P1_NON_CONFIRM: u8 = 0x00;
const P1_CONFIRM: u8 = 0x01;
const P2_EXTEND: u8 = 0x01;
const P2_MORE: u8 = 0x02;

#[derive(Debug, Clone)]
pub struct AppConfigResponse {
    pub blind_signing_enabled: bool,
    pub pubkey_display_mode: u8,
    pub version: [u8; 3],
}

pub fn encode_apdu(instruction: AppInstruction, p1: u8, p2: u8, data: &[u8]) -> Result<Vec<u8>> {
    if data.len() > u8::MAX as usize {
        bail!("APDU payload too large: {}", data.len());
    }

    let mut out = Vec::with_capacity(5 + data.len());
    out.extend_from_slice(&[
        crate::constants::APP_CLA,
        instruction as u8,
        p1,
        p2,
        data.len() as u8,
    ]);
    out.extend_from_slice(data);
    Ok(out)
}

pub fn decode_apdu_response(response: &[u8]) -> Result<(&[u8], u16)> {
    if response.len() < 2 {
        bail!("APDU response too short");
    }
    let split = response.len() - 2;
    Ok((
        &response[..split],
        u16::from_be_bytes([response[split], response[split + 1]]),
    ))
}

pub fn build_get_app_config_apdu() -> Result<Vec<u8>> {
    encode_apdu(AppInstruction::GetAppConfig, 0, 0, &[])
}

pub fn decode_get_app_config_response(response: &[u8]) -> Result<AppConfigResponse> {
    if response.len() != 5 {
        bail!("unexpected app config response length: {}", response.len());
    }

    Ok(AppConfigResponse {
        blind_signing_enabled: response[0] != 0,
        pubkey_display_mode: response[1],
        version: [response[2], response[3], response[4]],
    })
}

pub fn build_get_pubkey_apdu(derivation_path: &[u32], display: bool) -> Result<Vec<u8>> {
    let payload = serialize_derivation_path(derivation_path)?;
    encode_apdu(
        AppInstruction::GetPubkey,
        if display { P1_CONFIRM } else { P1_NON_CONFIRM },
        0,
        &payload,
    )
}

pub fn decode_get_pubkey_response(response: &[u8]) -> Result<[u8; 32]> {
    if response.len() != 32 {
        bail!("unexpected pubkey response length: {}", response.len());
    }

    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(response);
    Ok(pubkey)
}

pub fn build_sign_message_apdus(derivation_path: &[u32], message: &[u8]) -> Result<Vec<Vec<u8>>> {
    if message.is_empty() {
        bail!("message cannot be empty");
    }

    let mut payload = Vec::with_capacity(1 + derivation_path.len() * 4 + message.len());
    payload.push(SOLANA_PAYLOAD_VERSION);
    payload.extend_from_slice(&serialize_derivation_path(derivation_path)?);
    payload.extend_from_slice(message);

    let mut apdus = Vec::new();
    let mut offset = 0usize;
    while offset < payload.len() {
        let end = (offset + u8::MAX as usize).min(payload.len());
        let is_first = offset == 0;
        let is_last = end == payload.len();
        let mut p2 = 0u8;
        if !is_first {
            p2 |= P2_EXTEND;
        }
        if !is_last {
            p2 |= P2_MORE;
        }

        apdus.push(encode_apdu(
            AppInstruction::SignMessage,
            P1_CONFIRM,
            p2,
            &payload[offset..end],
        )?);
        offset = end;
    }

    Ok(apdus)
}

pub fn decode_sign_message_response(response: &[u8]) -> Result<[u8; 64]> {
    if response.len() != 64 {
        bail!("unexpected sign response length: {}", response.len());
    }

    let mut signature = [0u8; 64];
    signature.copy_from_slice(response);
    Ok(signature)
}

#[cfg(test)]
mod tests {
    use super::{build_get_app_config_apdu, build_get_pubkey_apdu, build_sign_message_apdus};

    #[test]
    fn encodes_get_app_config_apdu() {
        let apdu = build_get_app_config_apdu().unwrap();
        assert_eq!(hex::encode(apdu), "e004000000");
    }

    #[test]
    fn encodes_get_pubkey_apdu() {
        let apdu = build_get_pubkey_apdu(&[0x8000_002c, 0x8000_01f5], true).unwrap();
        assert_eq!(hex::encode(apdu), "e005010009028000002c800001f5");
    }

    #[test]
    fn chunks_sign_message_apdus_with_expected_flags() {
        let message = vec![0xabu8; 300];
        let apdus = build_sign_message_apdus(&[0x8000_002c, 0x8000_01f5], &message).unwrap();

        assert_eq!(apdus.len(), 2);
        assert_eq!(apdus[0][0..4], [0xe0, 0x06, 0x01, 0x02]);
        assert_eq!(apdus[1][0..4], [0xe0, 0x06, 0x01, 0x01]);
    }
}
