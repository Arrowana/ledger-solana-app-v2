import { AppInstruction, APP_CLA, AppP1, type ProposalVote } from "./constants.js";
import { serializeDerivationPath } from "./derivation.js";

export type SavedEntry = {
  slot: number;
  multisig: Buffer;
  member: Buffer;
  path: number[];
};

export type ProposalVoteResponse = {
  signature: Buffer;
  member: Buffer;
  proposal: Buffer;
  messageHash: Buffer;
};

export type ProposalCreateUpgradeResponse = {
  createSignature: Buffer;
  proposalSignature: Buffer;
  intentHash: Buffer;
  createMessageHash: Buffer;
  proposalMessageHash: Buffer;
};

export type ProposalExecuteUpgradeResponse = {
  signature: Buffer;
  intentHash: Buffer;
  messageHash: Buffer;
};

export function encodeApdu(
  instruction: AppInstruction,
  p1 = AppP1.Confirm,
  p2 = 0,
  data = Buffer.alloc(0),
): Buffer {
  if (data.length > 255) {
    throw new Error(`APDU payload too large: ${data.length}`);
  }

  return Buffer.concat([
    Buffer.from([APP_CLA, instruction, p1, p2, data.length]),
    data,
  ]);
}

export function decodeApduResponse(response: Buffer): { data: Buffer; statusWord: number } {
  if (response.length < 2) {
    throw new Error("APDU response too short");
  }

  return {
    data: response.subarray(0, response.length - 2),
    statusWord: response.readUInt16BE(response.length - 2),
  };
}

export function buildSaveMultisigApdu(args: {
  multisig: Buffer;
  derivationPath: readonly number[];
  nonConfirm: boolean;
}): Buffer {
  const payload = Buffer.concat([
    serializeDerivationPath(args.derivationPath),
    args.multisig,
  ]);
  return encodeApdu(
    AppInstruction.SaveMultisig,
    args.nonConfirm ? AppP1.NonConfirm : AppP1.Confirm,
    0,
    payload,
  );
}

export function decodeSaveMultisigResponse(response: Buffer): { slot: number; member: Buffer } {
  if (response.length !== 33) {
    throw new Error(`Unexpected save response length: ${response.length}`);
  }

  return {
    slot: response[0],
    member: response.subarray(1),
  };
}

export function buildListMultisigSlotApdu(slot: number): Buffer {
  if (slot < 0 || slot > 255) {
    throw new Error(`Invalid slot: ${slot}`);
  }
  return encodeApdu(AppInstruction.ListMultisigSlot, 0, slot);
}

export function decodeListMultisigSlotResponse(slot: number, response: Buffer): SavedEntry | null {
  if (response.length === 1 && response[0] === 0) {
    return null;
  }

  if (response.length < 67) {
    throw new Error(`Unexpected list response length: ${response.length}`);
  }

  if (response[0] !== 1) {
    throw new Error(`Unexpected slot occupancy marker: ${response[0]}`);
  }

  const pathLength = response[65];
  const expectedLength = 66 + pathLength * 4;
  if (response.length !== expectedLength) {
    throw new Error(`Unexpected list response path payload length: ${response.length}`);
  }

  const path: number[] = [];
  for (let index = 0; index < pathLength; index += 1) {
    path.push(response.readUInt32BE(66 + index * 4));
  }

  return {
    slot,
    multisig: response.subarray(1, 33),
    member: response.subarray(33, 65),
    path,
  };
}

export function buildProposalVoteApdu(args: {
  multisig: Buffer;
  transactionIndex: bigint;
  vote: ProposalVote;
  blockhash: Buffer;
  feePayer?: Buffer;
  nonConfirm: boolean;
}): Buffer {
  const txIndex = Buffer.alloc(8);
  txIndex.writeBigUInt64LE(args.transactionIndex);

  const feePayerFlag = args.feePayer ? 1 : 0;
  const payloadParts = [
    args.multisig,
    txIndex,
    Buffer.from([args.vote]),
    args.blockhash,
    Buffer.from([feePayerFlag]),
  ];

  if (args.feePayer) {
    payloadParts.push(args.feePayer);
  }

  return encodeApdu(
    AppInstruction.ProposalVote,
    args.nonConfirm ? AppP1.NonConfirm : AppP1.Confirm,
    0,
    Buffer.concat(payloadParts),
  );
}

export function decodeProposalVoteResponse(response: Buffer): ProposalVoteResponse {
  if (response.length !== 160) {
    throw new Error(`Unexpected proposal-vote response length: ${response.length}`);
  }

  return {
    signature: response.subarray(0, 64),
    member: response.subarray(64, 96),
    proposal: response.subarray(96, 128),
    messageHash: response.subarray(128, 160),
  };
}

export function buildProposalCreateUpgradeApdu(args: {
  multisig: Buffer;
  transactionIndex: bigint;
  vaultIndex: number;
  program: Buffer;
  buffer: Buffer;
  spill: Buffer;
  transactionBlockhash: Buffer;
  proposalBlockhash: Buffer;
  nonConfirm: boolean;
}): Buffer {
  const txIndex = Buffer.alloc(8);
  txIndex.writeBigUInt64LE(args.transactionIndex);

  return encodeApdu(
    AppInstruction.ProposalCreateUpgrade,
    args.nonConfirm ? AppP1.NonConfirm : AppP1.Confirm,
    0,
    Buffer.concat([
      args.multisig,
      txIndex,
      Buffer.from([args.vaultIndex]),
      args.program,
      args.buffer,
      args.spill,
      args.transactionBlockhash,
      args.proposalBlockhash,
    ]),
  );
}

export function decodeProposalCreateUpgradeResponse(
  response: Buffer,
): ProposalCreateUpgradeResponse {
  if (response.length !== 224) {
    throw new Error(`Unexpected proposal-create-upgrade response length: ${response.length}`);
  }

  return {
    createSignature: response.subarray(0, 64),
    proposalSignature: response.subarray(64, 128),
    intentHash: response.subarray(128, 160),
    createMessageHash: response.subarray(160, 192),
    proposalMessageHash: response.subarray(192, 224),
  };
}

export function buildProposalExecuteUpgradeApdu(args: {
  multisig: Buffer;
  transactionIndex: bigint;
  vaultIndex: number;
  program: Buffer;
  buffer: Buffer;
  spill: Buffer;
  blockhash: Buffer;
  nonConfirm: boolean;
}): Buffer {
  const txIndex = Buffer.alloc(8);
  txIndex.writeBigUInt64LE(args.transactionIndex);

  return encodeApdu(
    AppInstruction.ProposalExecuteUpgrade,
    args.nonConfirm ? AppP1.NonConfirm : AppP1.Confirm,
    0,
    Buffer.concat([
      args.multisig,
      txIndex,
      Buffer.from([args.vaultIndex]),
      args.program,
      args.buffer,
      args.spill,
      args.blockhash,
    ]),
  );
}

export function decodeProposalExecuteUpgradeResponse(
  response: Buffer,
): ProposalExecuteUpgradeResponse {
  if (response.length !== 128) {
    throw new Error(`Unexpected proposal-execute-upgrade response length: ${response.length}`);
  }

  return {
    signature: response.subarray(0, 64),
    intentHash: response.subarray(64, 96),
    messageHash: response.subarray(96, 128),
  };
}

export function buildResetMultisigsApdu(nonConfirm: boolean): Buffer {
  return encodeApdu(
    AppInstruction.ResetMultisigs,
    nonConfirm ? AppP1.NonConfirm : AppP1.Confirm,
  );
}
