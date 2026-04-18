extern crate alloc;

use alloc::vec::Vec;
use borsh::BorshDeserialize;

use crate::AppSW;

pub const ACCOUNT_DISCRIMINATOR_LEN: usize = 8;
pub const PUBKEY_LEN: usize = 32;
pub const VOTE_PERMISSION_MASK: u8 = 1 << 1;

pub type PubkeyBytes = [u8; PUBKEY_LEN];

#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize)]
pub struct Permissions {
    pub mask: u8,
}

impl Permissions {
    pub fn has_vote(&self) -> bool {
        (self.mask & VOTE_PERMISSION_MASK) != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize)]
pub struct Member {
    pub key: PubkeyBytes,
    pub permissions: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize)]
pub struct MultisigAccount {
    pub create_key: PubkeyBytes,
    pub config_authority: PubkeyBytes,
    pub threshold: u16,
    pub time_lock: u32,
    pub transaction_index: u64,
    pub stale_transaction_index: u64,
    pub rent_collector: Option<PubkeyBytes>,
    pub bump: u8,
    pub members: Vec<Member>,
}

impl MultisigAccount {
    pub fn find_member(&self, member: &PubkeyBytes) -> Option<&Member> {
        self.members
            .iter()
            .find(|candidate| &candidate.key == member)
    }

    pub fn member_can_vote(&self, member: &PubkeyBytes) -> bool {
        self.find_member(member)
            .map(|candidate| candidate.permissions.has_vote())
            .unwrap_or(false)
    }

    pub fn try_from_account_data(data: &[u8]) -> Result<Self, AppSW> {
        let bytes = data
            .get(ACCOUNT_DISCRIMINATOR_LEN..)
            .ok_or(AppSW::InvalidData)?;
        Self::try_from_slice(bytes).map_err(|_| AppSW::InvalidData)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize)]
pub enum ProposalStatus {
    Draft { timestamp: i64 },
    Active { timestamp: i64 },
    Rejected { timestamp: i64 },
    Approved { timestamp: i64 },
    Executing,
    Executed { timestamp: i64 },
    Cancelled { timestamp: i64 },
}

impl ProposalStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshDeserialize)]
pub struct ProposalAccount {
    pub multisig: PubkeyBytes,
    pub transaction_index: u64,
    pub status: ProposalStatus,
    pub bump: u8,
    pub approved: Vec<PubkeyBytes>,
    pub rejected: Vec<PubkeyBytes>,
    pub cancelled: Vec<PubkeyBytes>,
}

impl ProposalAccount {
    pub fn try_from_account_data(data: &[u8]) -> Result<Self, AppSW> {
        let bytes = data
            .get(ACCOUNT_DISCRIMINATOR_LEN..)
            .ok_or(AppSW::InvalidData)?;
        Self::try_from_slice(bytes).map_err(|_| AppSW::InvalidData)
    }
}
