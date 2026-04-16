import { address, type Address } from "@solana/kit";

export const SQUADS_PROGRAM_ADDRESS = address(
  "SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf",
);
export const SQUADS_PROGRAM_ID: Address = SQUADS_PROGRAM_ADDRESS;
export const SYSTEM_PROGRAM_ADDRESS = address(
  "11111111111111111111111111111111",
);
export const BPF_LOADER_UPGRADEABLE_PROGRAM_ADDRESS = address(
  "BPFLoaderUpgradeab1e11111111111111111111111",
);
export const SYSVAR_RENT_ADDRESS = address(
  "SysvarRent111111111111111111111111111111111",
);
export const SYSVAR_CLOCK_ADDRESS = address(
  "SysvarC1ock11111111111111111111111111111111",
);

export const VOTE_PERMISSION_MASK = 1 << 1;
export const MAX_SAVED_MULTISIGS = 8;

export const APP_CLA = 0xe0;
export const SW_OK = 0x9000;
export const SW_USER_REFUSED = 0x6985;
export const SW_NOT_FOUND = 0x6a88;
export const SW_INVALID_DATA = 0x6a80;
export const SW_CONDITIONS_NOT_SATISFIED = 0x6986;

export enum AppInstruction {
  GetVersion = 0x00,
  SaveMultisig = 0x10,
  ListMultisigSlot = 0x11,
  ProposalVote = 0x12,
  ResetMultisigs = 0x13,
  ProposalCreateUpgrade = 0x14,
  ProposalExecuteUpgrade = 0x15,
}

export enum AppP1 {
  Confirm = 0x00,
  NonConfirm = 0x01,
}

export enum ProposalVote {
  Approve = 0x00,
  Reject = 0x01,
}

export type TransportKind = "hid" | "speculos";
