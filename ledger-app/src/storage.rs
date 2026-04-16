use ledger_device_sdk::nvm::{AtomicStorage, SingleStorage};
use ledger_device_sdk::NVMData;

pub const PUBKEY_LENGTH: usize = 32;
pub const MAX_DERIVATION_PATH_LENGTH: usize = 5;
pub const MAX_SAVED_MULTISIGS: usize = 8;
pub const MAX_REVIEWED_UPGRADE_INTENTS: usize = 8;
pub const MESSAGE_HASH_LENGTH: usize = 32;

const VERSION: u8 = 1;
const STORAGE_SIZE: usize = 4096;
const ENTRY_SIZE: usize = 1 + PUBKEY_LENGTH + PUBKEY_LENGTH + 1 + MAX_DERIVATION_PATH_LENGTH * 4;
const REVIEWED_UPGRADE_SIZE: usize =
    1 + PUBKEY_LENGTH + 8 + 1 + PUBKEY_LENGTH * 3 + MESSAGE_HASH_LENGTH;

#[link_section = ".nvm_data"]
static mut DATA: NVMData<AtomicStorage<[u8; STORAGE_SIZE]>> =
    NVMData::new(AtomicStorage::new(&[0u8; STORAGE_SIZE]));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SavedMultisigEntry {
    pub occupied: bool,
    pub multisig: [u8; PUBKEY_LENGTH],
    pub member: [u8; PUBKEY_LENGTH],
    pub path_length: u8,
    pub derivation_path: [u32; MAX_DERIVATION_PATH_LENGTH],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReviewedUpgradeIntent {
    pub occupied: bool,
    pub multisig: [u8; PUBKEY_LENGTH],
    pub transaction_index: u64,
    pub vault_index: u8,
    pub program: [u8; PUBKEY_LENGTH],
    pub buffer: [u8; PUBKEY_LENGTH],
    pub spill: [u8; PUBKEY_LENGTH],
    pub intent_hash: [u8; MESSAGE_HASH_LENGTH],
}

#[derive(Clone, Copy)]
pub struct Storage;

impl Default for Storage {
    fn default() -> Self {
        Self
    }
}

impl Storage {
    pub fn init(&self) {
        let mut data = self.load();
        if data[0] != VERSION {
            data = Self::empty_bytes();
            self.write_raw(&data);
        }
    }

    pub fn find_multisig(&self, multisig: &[u8; PUBKEY_LENGTH]) -> Option<u8> {
        for slot in 0..MAX_SAVED_MULTISIGS {
            if let Some(entry) = self.read_slot(slot as u8) {
                if &entry.multisig == multisig {
                    return Some(slot as u8);
                }
            }
        }
        None
    }

    pub fn find_free_slot(&self) -> Option<u8> {
        for slot in 0..MAX_SAVED_MULTISIGS {
            if self.read_slot(slot as u8).is_none() {
                return Some(slot as u8);
            }
        }
        None
    }

    pub fn read_slot(&self, slot: u8) -> Option<SavedMultisigEntry> {
        if slot as usize >= MAX_SAVED_MULTISIGS {
            return None;
        }
        let data = self.load();
        let offset = Self::entry_offset(slot as usize);
        Self::decode_entry(&data[offset..offset + ENTRY_SIZE])
    }

    pub fn upsert(&self, entry: &SavedMultisigEntry) -> Option<u8> {
        let slot = self
            .find_multisig(&entry.multisig)
            .or_else(|| self.find_free_slot())?;
        let mut data = self.load();
        let offset = Self::entry_offset(slot as usize);
        Self::encode_entry(entry, &mut data[offset..offset + ENTRY_SIZE]);
        self.write_raw(&data);
        Some(slot)
    }

    pub fn find_reviewed_upgrade(
        &self,
        multisig: &[u8; PUBKEY_LENGTH],
        transaction_index: u64,
    ) -> Option<u8> {
        for slot in 0..MAX_REVIEWED_UPGRADE_INTENTS {
            if let Some(entry) = self.read_reviewed_upgrade(slot as u8) {
                if &entry.multisig == multisig && entry.transaction_index == transaction_index {
                    return Some(slot as u8);
                }
            }
        }
        None
    }

    pub fn read_reviewed_upgrade(&self, slot: u8) -> Option<ReviewedUpgradeIntent> {
        if slot as usize >= MAX_REVIEWED_UPGRADE_INTENTS {
            return None;
        }
        let data = self.load();
        let offset = Self::reviewed_upgrade_offset(slot as usize);
        Self::decode_reviewed_upgrade(&data[offset..offset + REVIEWED_UPGRADE_SIZE])
    }

    pub fn upsert_reviewed_upgrade(&self, entry: &ReviewedUpgradeIntent) -> Option<u8> {
        let slot = self
            .find_reviewed_upgrade(&entry.multisig, entry.transaction_index)
            .or_else(|| {
                (0..MAX_REVIEWED_UPGRADE_INTENTS)
                    .find(|idx| self.read_reviewed_upgrade(*idx as u8).is_none())
                    .map(|idx| idx as u8)
            })?;

        let mut data = self.load();
        let offset = Self::reviewed_upgrade_offset(slot as usize);
        Self::encode_reviewed_upgrade(entry, &mut data[offset..offset + REVIEWED_UPGRADE_SIZE]);
        self.write_raw(&data);
        Some(slot)
    }

    pub fn reset(&self) {
        let data = Self::empty_bytes();
        self.write_raw(&data);
    }

    fn load(&self) -> [u8; STORAGE_SIZE] {
        let data = &raw const DATA;
        let storage = unsafe { (*data).get_ref() };
        *storage.get_ref()
    }

    fn write_raw(&self, data: &[u8; STORAGE_SIZE]) {
        let data_ref = &raw mut DATA;
        let storage = unsafe { (*data_ref).get_mut() };
        storage.update(data);
    }

    fn empty_bytes() -> [u8; STORAGE_SIZE] {
        let mut data = [0u8; STORAGE_SIZE];
        data[0] = VERSION;
        data
    }

    const fn entry_offset(slot: usize) -> usize {
        1 + slot * ENTRY_SIZE
    }

    const fn reviewed_upgrade_offset(slot: usize) -> usize {
        1 + MAX_SAVED_MULTISIGS * ENTRY_SIZE + slot * REVIEWED_UPGRADE_SIZE
    }

    fn decode_entry(bytes: &[u8]) -> Option<SavedMultisigEntry> {
        if bytes.first().copied() != Some(1) {
            return None;
        }

        let mut multisig = [0u8; PUBKEY_LENGTH];
        multisig.copy_from_slice(&bytes[1..33]);
        let mut member = [0u8; PUBKEY_LENGTH];
        member.copy_from_slice(&bytes[33..65]);
        let path_length = bytes[65];
        if path_length as usize > MAX_DERIVATION_PATH_LENGTH {
            return None;
        }

        let mut derivation_path = [0u32; MAX_DERIVATION_PATH_LENGTH];
        let mut offset = 66;
        for item in derivation_path.iter_mut().take(path_length as usize) {
            *item = u32::from_be_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]);
            offset += 4;
        }

        Some(SavedMultisigEntry {
            occupied: true,
            multisig,
            member,
            path_length,
            derivation_path,
        })
    }

    fn encode_entry(entry: &SavedMultisigEntry, out: &mut [u8]) {
        out.fill(0);
        out[0] = u8::from(entry.occupied);
        out[1..33].copy_from_slice(&entry.multisig);
        out[33..65].copy_from_slice(&entry.member);
        out[65] = entry.path_length;
        let mut offset = 66;
        for index in 0..MAX_DERIVATION_PATH_LENGTH {
            out[offset..offset + 4].copy_from_slice(&entry.derivation_path[index].to_be_bytes());
            offset += 4;
        }
    }

    fn decode_reviewed_upgrade(bytes: &[u8]) -> Option<ReviewedUpgradeIntent> {
        if bytes.first().copied() != Some(1) {
            return None;
        }

        let mut multisig = [0u8; PUBKEY_LENGTH];
        multisig.copy_from_slice(&bytes[1..33]);
        let transaction_index = u64::from_le_bytes(bytes[33..41].try_into().ok()?);
        let vault_index = bytes[41];

        let mut program = [0u8; PUBKEY_LENGTH];
        program.copy_from_slice(&bytes[42..74]);
        let mut buffer = [0u8; PUBKEY_LENGTH];
        buffer.copy_from_slice(&bytes[74..106]);
        let mut spill = [0u8; PUBKEY_LENGTH];
        spill.copy_from_slice(&bytes[106..138]);
        let mut intent_hash = [0u8; MESSAGE_HASH_LENGTH];
        intent_hash.copy_from_slice(&bytes[138..170]);

        Some(ReviewedUpgradeIntent {
            occupied: true,
            multisig,
            transaction_index,
            vault_index,
            program,
            buffer,
            spill,
            intent_hash,
        })
    }

    fn encode_reviewed_upgrade(entry: &ReviewedUpgradeIntent, out: &mut [u8]) {
        out.fill(0);
        out[0] = u8::from(entry.occupied);
        out[1..33].copy_from_slice(&entry.multisig);
        out[33..41].copy_from_slice(&entry.transaction_index.to_le_bytes());
        out[41] = entry.vault_index;
        out[42..74].copy_from_slice(&entry.program);
        out[74..106].copy_from_slice(&entry.buffer);
        out[106..138].copy_from_slice(&entry.spill);
        out[138..170].copy_from_slice(&entry.intent_hash);
    }
}
