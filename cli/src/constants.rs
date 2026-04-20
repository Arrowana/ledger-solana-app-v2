pub const APP_CLA: u8 = 0xe0;
pub const SW_OK: u16 = 0x9000;
pub const SW_USER_REFUSED: u16 = 0x6985;
pub const HID_CHANNEL: u16 = 0x0101;
pub const HID_TAG_APDU: u8 = 0x05;
pub const LEDGER_VENDOR_ID: u16 = 0x2c97;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum AppInstruction {
    GetAppConfig = 0x04,
    GetPubkey = 0x05,
    SignMessage = 0x06,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportKind {
    Hid,
    Speculos,
}

impl TransportKind {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "hid" => Ok(Self::Hid),
            "speculos" => Ok(Self::Speculos),
            other => Err(anyhow::anyhow!("unsupported transport: {other}")),
        }
    }
}
