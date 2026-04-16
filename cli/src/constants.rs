pub const SQUADS_PROGRAM_ID: &str = "SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf";
pub const MAX_SAVED_MULTISIGS: u8 = 8;
pub const APP_CLA: u8 = 0xe0;
pub const SW_OK: u16 = 0x9000;
pub const SW_USER_REFUSED: u16 = 0x6985;
pub const SW_NOT_FOUND: u16 = 0x6a88;
pub const SW_INVALID_DATA: u16 = 0x6a80;
pub const SW_CONDITIONS_NOT_SATISFIED: u16 = 0x6986;
pub const HID_CHANNEL: u16 = 0x0101;
pub const HID_TAG_APDU: u8 = 0x05;
pub const LEDGER_VENDOR_ID: u16 = 0x2c97;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum AppInstruction {
    GetVersion = 0x00,
    SaveMultisig = 0x10,
    ListMultisigSlot = 0x11,
    ProposalVote = 0x12,
    ResetMultisigs = 0x13,
    ProposalCreateUpgrade = 0x14,
    ProposalExecuteUpgrade = 0x15,
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum ProposalVote {
    Approve = 0x00,
    Reject = 0x01,
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

