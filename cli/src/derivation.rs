use anyhow::{anyhow, bail, Result};

const HARDENED_OFFSET: u32 = 0x8000_0000;

pub fn parse_derivation_path(path: &str) -> Result<Vec<u32>> {
    if !path.starts_with("m/") {
        bail!("invalid derivation path: {path}");
    }

    let parts = path
        .trim_start_matches("m/")
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() || parts.len() > 5 {
        bail!("unsupported derivation path length: {path}");
    }

    let mut segments = Vec::with_capacity(parts.len());
    for part in parts {
        let hardened = part.ends_with('\'');
        let value_part = if hardened {
            &part[..part.len() - 1]
        } else {
            part
        };

        let value = value_part
            .parse::<u32>()
            .map_err(|_| anyhow!("invalid derivation segment: {part}"))?;
        if value >= HARDENED_OFFSET {
            bail!("invalid derivation value: {part}");
        }

        segments.push(if hardened {
            value | HARDENED_OFFSET
        } else {
            value
        });
    }

    if segments.len() < 2
        || segments[0] != (44 | HARDENED_OFFSET)
        || segments[1] != (501 | HARDENED_OFFSET)
    {
        bail!("only Solana derivation paths under m/44'/501' are supported");
    }

    Ok(segments)
}

pub fn serialize_derivation_path(segments: &[u32]) -> Result<Vec<u8>> {
    if segments.is_empty() || segments.len() > 5 {
        bail!("derivation path must contain 1-5 segments");
    }

    let mut out = Vec::with_capacity(1 + segments.len() * 4);
    out.push(segments.len() as u8);
    for segment in segments {
        out.extend_from_slice(&segment.to_be_bytes());
    }
    Ok(out)
}

pub fn format_derivation_path(segments: &[u32]) -> String {
    let parts = segments
        .iter()
        .map(|segment| {
            let hardened = (segment & HARDENED_OFFSET) != 0;
            let value = segment & !HARDENED_OFFSET;
            if hardened {
                format!("{value}'")
            } else {
                value.to_string()
            }
        })
        .collect::<Vec<_>>();
    format!("m/{}", parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::{format_derivation_path, parse_derivation_path, serialize_derivation_path};

    #[test]
    fn parses_solana_path() {
        let path = parse_derivation_path("m/44'/501'/0'/0'").unwrap();
        assert_eq!(
            path,
            vec![0x8000_002c, 0x8000_01f5, 0x8000_0000, 0x8000_0000]
        );
        assert_eq!(format_derivation_path(&path), "m/44'/501'/0'/0'");
    }

    #[test]
    fn serializes_path() {
        let encoded = serialize_derivation_path(&[0x8000_002c, 0x8000_01f5]).unwrap();
        assert_eq!(hex::encode(encoded), "028000002c800001f5");
    }
}
