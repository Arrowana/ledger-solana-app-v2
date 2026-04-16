import { createHash } from "node:crypto";
import { Buffer } from "node:buffer";

import {
  createSolanaRpc,
  getAddressDecoder,
  getAddressEncoder,
  getProgramDerivedAddress,
  type Address,
  type GetAccountInfoApi,
  type GetLatestBlockhashApi,
  type Rpc,
  type SendTransactionApi,
} from "@solana/kit";
import {
  PublicKey,
  TransactionInstruction,
  TransactionMessage as Web3TransactionMessage,
} from "@solana/web3.js";
import { generated as sqdsGenerated, instructions as sqdsInstructions } from "@sqds/multisig";

import {
  BPF_LOADER_UPGRADEABLE_PROGRAM_ADDRESS,
  ProposalVote,
  SQUADS_PROGRAM_ADDRESS,
  SYSVAR_CLOCK_ADDRESS,
  SYSVAR_RENT_ADDRESS,
  VOTE_PERMISSION_MASK,
} from "./constants.js";

export type SolanaRpcClient = Rpc<GetAccountInfoApi & GetLatestBlockhashApi & SendTransactionApi>;
type DecodedRpcAccount = {
  data: Buffer;
  executable: boolean;
  owner: Address;
  lamports: bigint;
  space: bigint;
};

type DecodedMember = {
  key: Address;
  permissionsMask: number;
};

export type DecodedMultisig = {
  threshold: number;
  members: DecodedMember[];
};

export type ProposalStatusKind =
  | "Draft"
  | "Active"
  | "Rejected"
  | "Approved"
  | "Executing"
  | "Executed"
  | "Cancelled";

export type DecodedProposal = {
  multisig: Address;
  transactionIndex: bigint;
  status: ProposalStatusKind;
};

export type PreflightProposalVote = {
  multisig: DecodedMultisig;
  proposal: DecodedProposal;
  proposalPda: Address;
  member: Address;
};

export type BuiltLegacyTransaction = {
  message: Buffer;
  messageHash: Buffer;
};

export type BuiltUpgradeCreateTransactions = {
  intentHash: Buffer;
  vaultPda: Address;
  transactionPda: Address;
  proposalPda: Address;
  programDataPda: Address;
  create: BuiltLegacyTransaction;
  proposal: BuiltLegacyTransaction;
};

export type BuiltUpgradeExecuteTransaction = {
  intentHash: Buffer;
  vaultPda: Address;
  transactionPda: Address;
  proposalPda: Address;
  programDataPda: Address;
  message: Buffer;
  messageHash: Buffer;
};

const addressEncoder = getAddressEncoder();
const addressDecoder = getAddressDecoder();
const upgradeableLoaderUpgradeData = Uint8Array.from([3, 0, 0, 0]);
const multisigDiscriminator = Buffer.from([224, 116, 121, 186, 68, 161, 79, 236]);
const proposalDiscriminator = Buffer.from([26, 94, 189, 187, 116, 136, 53, 33]);

function toPublicKey(value: Address): PublicKey {
  return new PublicKey(value);
}

function toLegacyMessageBytes(args: {
  payerKey: Address;
  recentBlockhash: string;
  instructions: readonly TransactionInstruction[];
}): Buffer {
  const message = new Web3TransactionMessage({
    payerKey: toPublicKey(args.payerKey),
    recentBlockhash: args.recentBlockhash,
    instructions: [...args.instructions],
  }).compileToLegacyMessage();
  return Buffer.from(message.serialize());
}

export function createConnection(url: string): SolanaRpcClient {
  return createSolanaRpc(url);
}

async function fetchAccount(connection: SolanaRpcClient, accountAddress: Address) {
  const response = await connection
    .getAccountInfo(accountAddress, { commitment: "confirmed", encoding: "base64" })
    .send();
  return response.value;
}

function decodeRpcAccount(
  accountInfo: Exclude<Awaited<ReturnType<typeof fetchAccount>>, null>,
): DecodedRpcAccount {
  return {
    data: Buffer.from(accountInfo.data[0], "base64"),
    executable: accountInfo.executable,
    owner: accountInfo.owner,
    lamports: accountInfo.lamports,
    space: accountInfo.space,
  };
}

function assertDiscriminator(data: Buffer, expected: Buffer, label: string): void {
  if (data.length < 8 || !data.subarray(0, 8).equals(expected)) {
    throw new Error(`Invalid ${label} account discriminator`);
  }
}

function readAddress(data: Buffer, offset: number): { value: Address; offset: number } {
  return {
    value: addressDecoder.decode(data.subarray(offset, offset + 32)),
    offset: offset + 32,
  };
}

function readU16(data: Buffer, offset: number): { value: number; offset: number } {
  return {
    value: data.readUInt16LE(offset),
    offset: offset + 2,
  };
}

function readU32(data: Buffer, offset: number): { value: number; offset: number } {
  return {
    value: data.readUInt32LE(offset),
    offset: offset + 4,
  };
}

function readU64(data: Buffer, offset: number): { value: bigint; offset: number } {
  return {
    value: data.readBigUInt64LE(offset),
    offset: offset + 8,
  };
}

function skipProposalStatusPayload(
  variant: number,
  offset: number,
): { status: ProposalStatusKind; offset: number } {
  switch (variant) {
    case 0:
      return { status: "Draft", offset: offset + 8 };
    case 1:
      return { status: "Active", offset: offset + 8 };
    case 2:
      return { status: "Rejected", offset: offset + 8 };
    case 3:
      return { status: "Approved", offset: offset + 8 };
    case 4:
      return { status: "Executing", offset };
    case 5:
      return { status: "Executed", offset: offset + 8 };
    case 6:
      return { status: "Cancelled", offset: offset + 8 };
    default:
      throw new Error(`Unknown proposal status variant: ${variant}`);
  }
}

function decodeMultisigAccount(data: Buffer): DecodedMultisig {
  assertDiscriminator(data, multisigDiscriminator, "multisig");

  let offset = 8;
  offset += 32;
  offset += 32;

  const threshold = readU16(data, offset);
  offset = threshold.offset;
  offset += 4;
  offset += 8;
  offset += 8;
  offset += 1;
  offset += 32;
  offset += 1;

  const membersLength = readU32(data, offset);
  offset = membersLength.offset;

  const members: DecodedMember[] = [];
  for (let index = 0; index < membersLength.value; index += 1) {
    const member = readAddress(data, offset);
    offset = member.offset;
    members.push({
      key: member.value,
      permissionsMask: data[offset],
    });
    offset += 1;
  }

  return {
    threshold: threshold.value,
    members,
  };
}

function decodeProposalAccount(data: Buffer): DecodedProposal {
  assertDiscriminator(data, proposalDiscriminator, "proposal");

  let offset = 8;
  const multisig = readAddress(data, offset);
  offset = multisig.offset;

  const transactionIndex = readU64(data, offset);
  offset = transactionIndex.offset;

  const variant = data[offset];
  offset += 1;

  const status = skipProposalStatusPayload(variant, offset);

  return {
    multisig: multisig.value,
    transactionIndex: transactionIndex.value,
    status: status.status,
  };
}

function encodeU8(value: number): Buffer {
  return Buffer.from([value & 0xff]);
}

function encodeU64(value: bigint): Buffer {
  const out = Buffer.alloc(8);
  out.writeBigUInt64LE(value);
  return out;
}

export async function getProposalPda(args: {
  multisigPda: Address;
  transactionIndex: bigint;
}): Promise<Address> {
  const [proposalPda] = await getProgramDerivedAddress({
    programAddress: SQUADS_PROGRAM_ADDRESS,
    seeds: [
      "multisig",
      addressEncoder.encode(args.multisigPda),
      "transaction",
      encodeU64(args.transactionIndex),
      "proposal",
    ],
  });
  return proposalPda;
}

export async function getTransactionPda(args: {
  multisigPda: Address;
  transactionIndex: bigint;
}): Promise<Address> {
  const [transactionPda] = await getProgramDerivedAddress({
    programAddress: SQUADS_PROGRAM_ADDRESS,
    seeds: [
      "multisig",
      addressEncoder.encode(args.multisigPda),
      "transaction",
      encodeU64(args.transactionIndex),
    ],
  });
  return transactionPda;
}

export async function getVaultPda(args: {
  multisigPda: Address;
  vaultIndex: number;
}): Promise<Address> {
  const [vaultPda] = await getProgramDerivedAddress({
    programAddress: SQUADS_PROGRAM_ADDRESS,
    seeds: ["multisig", addressEncoder.encode(args.multisigPda), "vault", encodeU8(args.vaultIndex)],
  });
  return vaultPda;
}

export async function getProgramDataPda(args: { program: Address }): Promise<Address> {
  const [programDataPda] = await getProgramDerivedAddress({
    programAddress: BPF_LOADER_UPGRADEABLE_PROGRAM_ADDRESS,
    seeds: [addressEncoder.encode(args.program)],
  });
  return programDataPda;
}

export async function fetchValidatedMultisig(
  connection: SolanaRpcClient,
  multisigPda: Address,
): Promise<DecodedMultisig> {
  const accountInfo = await fetchAccount(connection, multisigPda);
  if (!accountInfo) {
    throw new Error(`Multisig account not found: ${multisigPda}`);
  }
  if (accountInfo.owner !== SQUADS_PROGRAM_ADDRESS) {
    throw new Error(`Account is not owned by Squads v4: ${multisigPda}`);
  }

  return decodeMultisigAccount(decodeRpcAccount(accountInfo).data);
}

export async function preflightProposalVote(args: {
  connection: SolanaRpcClient;
  multisigPda: Address;
  transactionIndex: bigint;
  member: Address;
}): Promise<PreflightProposalVote> {
  const multisig = await fetchValidatedMultisig(args.connection, args.multisigPda);

  const memberRecord = multisig.members.find((candidate) => candidate.key === args.member);
  if (!memberRecord) {
    throw new Error(`Saved Ledger member is not part of the multisig: ${args.member}`);
  }
  if ((memberRecord.permissionsMask & VOTE_PERMISSION_MASK) === 0) {
    throw new Error(`Saved Ledger member lacks Vote permission: ${args.member}`);
  }

  const proposalPda = await getProposalPda({
    multisigPda: args.multisigPda,
    transactionIndex: args.transactionIndex,
  });

  const proposalInfo = await fetchAccount(args.connection, proposalPda);
  if (!proposalInfo) {
    throw new Error(`Proposal account not found: ${proposalPda}`);
  }
  if (proposalInfo.owner !== SQUADS_PROGRAM_ADDRESS) {
    throw new Error(`Proposal account is not owned by Squads v4: ${proposalPda}`);
  }

  const proposal = decodeProposalAccount(decodeRpcAccount(proposalInfo).data);
  if (proposal.multisig !== args.multisigPda) {
    throw new Error("Proposal account does not point back to the requested multisig");
  }
  if (proposal.transactionIndex !== args.transactionIndex) {
    throw new Error("Proposal transaction index does not match the requested index");
  }
  if (proposal.status !== "Active") {
    throw new Error(`Proposal is not Active; current status is ${proposal.status}`);
  }

  return {
    multisig,
    proposal,
    proposalPda,
    member: args.member,
  };
}

function encodeShortvec(value: number): Buffer {
  const bytes: number[] = [];
  let remaining = value >>> 0;
  do {
    let next = remaining & 0x7f;
    remaining >>>= 7;
    if (remaining !== 0) {
      next |= 0x80;
    }
    bytes.push(next);
  } while (remaining !== 0);
  return Buffer.from(bytes);
}

function buildHashedLegacyMessage(message: Buffer): BuiltLegacyTransaction {
  return {
    message,
    messageHash: createHash("sha256").update(message).digest(),
  };
}

export async function buildUpgradeIntentHash(args: {
  multisigPda: Address;
  vaultIndex: number;
  program: Address;
  buffer: Address;
  spill: Address;
}): Promise<Buffer> {
  const [vaultPda, programDataPda] = await Promise.all([
    getVaultPda({ multisigPda: args.multisigPda, vaultIndex: args.vaultIndex }),
    getProgramDataPda({ program: args.program }),
  ]);
  const upgradeInstruction = new TransactionInstruction({
    programId: toPublicKey(BPF_LOADER_UPGRADEABLE_PROGRAM_ADDRESS),
    keys: [
      { pubkey: toPublicKey(programDataPda), isSigner: false, isWritable: true },
      { pubkey: toPublicKey(args.program), isSigner: false, isWritable: true },
      { pubkey: toPublicKey(args.buffer), isSigner: false, isWritable: true },
      { pubkey: toPublicKey(args.spill), isSigner: false, isWritable: true },
      { pubkey: toPublicKey(SYSVAR_RENT_ADDRESS), isSigner: false, isWritable: false },
      { pubkey: toPublicKey(SYSVAR_CLOCK_ADDRESS), isSigner: false, isWritable: false },
      { pubkey: toPublicKey(vaultPda), isSigner: true, isWritable: false },
    ],
    data: Buffer.from(upgradeableLoaderUpgradeData),
  });
  const wrappedMessage = new Web3TransactionMessage({
    payerKey: toPublicKey(vaultPda),
    recentBlockhash: "11111111111111111111111111111111",
    instructions: [upgradeInstruction],
  });
  const instruction = sqdsInstructions.vaultTransactionCreate({
    multisigPda: toPublicKey(args.multisigPda),
    transactionIndex: 0n,
    creator: toPublicKey(vaultPda),
    vaultIndex: args.vaultIndex,
    ephemeralSigners: 0,
    transactionMessage: wrappedMessage,
    programId: toPublicKey(SQUADS_PROGRAM_ADDRESS),
  });
  const data = Buffer.from(instruction.data);
  const wrappedLength = data.readUInt32LE(10);
  const wrappedBytes = data.subarray(14, 14 + wrappedLength);
  return createHash("sha256").update(wrappedBytes).digest();
}

export async function buildProposalVoteTransaction(args: {
  member: Address;
  feePayer: Address;
  multisigPda: Address;
  transactionIndex: bigint;
  vote: ProposalVote;
  recentBlockhash: string;
  lastValidBlockHeight: bigint;
}): Promise<{ proposalPda: Address; message: Buffer; messageHash: Buffer }> {
  const proposalPda = await getProposalPda({
    multisigPda: args.multisigPda,
    transactionIndex: args.transactionIndex,
  });

  void args.lastValidBlockHeight;
  const instruction =
    args.vote === ProposalVote.Approve
      ? sqdsGenerated.createProposalApproveInstruction(
          {
            multisig: toPublicKey(args.multisigPda),
            member: toPublicKey(args.member),
            proposal: toPublicKey(proposalPda),
          },
          { args: { memo: null } },
          toPublicKey(SQUADS_PROGRAM_ADDRESS),
        )
      : sqdsGenerated.createProposalRejectInstruction(
          {
            multisig: toPublicKey(args.multisigPda),
            member: toPublicKey(args.member),
            proposal: toPublicKey(proposalPda),
          },
          { args: { memo: null } },
          toPublicKey(SQUADS_PROGRAM_ADDRESS),
        );
  const message = toLegacyMessageBytes({
    payerKey: args.feePayer,
    recentBlockhash: args.recentBlockhash,
    instructions: [instruction],
  });
  const messageHash = createHash("sha256").update(message).digest();

  return {
    proposalPda,
    message,
    messageHash,
  };
}

export async function buildProposalCreateUpgradeTransactions(args: {
  member: Address;
  multisigPda: Address;
  transactionIndex: bigint;
  vaultIndex: number;
  program: Address;
  buffer: Address;
  spill: Address;
  transactionBlockhash: string;
  proposalBlockhash: string;
  lastValidBlockHeight: bigint;
}): Promise<BuiltUpgradeCreateTransactions> {
  const [vaultPda, transactionPda, proposalPda, programDataPda, intentHash] = await Promise.all([
    getVaultPda({ multisigPda: args.multisigPda, vaultIndex: args.vaultIndex }),
    getTransactionPda({ multisigPda: args.multisigPda, transactionIndex: args.transactionIndex }),
    getProposalPda({ multisigPda: args.multisigPda, transactionIndex: args.transactionIndex }),
    getProgramDataPda({ program: args.program }),
    buildUpgradeIntentHash({
      multisigPda: args.multisigPda,
      vaultIndex: args.vaultIndex,
      program: args.program,
      buffer: args.buffer,
      spill: args.spill,
    }),
  ]);

  const upgradeInstruction = new TransactionInstruction({
    programId: toPublicKey(BPF_LOADER_UPGRADEABLE_PROGRAM_ADDRESS),
    keys: [
      { pubkey: toPublicKey(programDataPda), isSigner: false, isWritable: true },
      { pubkey: toPublicKey(args.program), isSigner: false, isWritable: true },
      { pubkey: toPublicKey(args.buffer), isSigner: false, isWritable: true },
      { pubkey: toPublicKey(args.spill), isSigner: false, isWritable: true },
      { pubkey: toPublicKey(SYSVAR_RENT_ADDRESS), isSigner: false, isWritable: false },
      { pubkey: toPublicKey(SYSVAR_CLOCK_ADDRESS), isSigner: false, isWritable: false },
      { pubkey: toPublicKey(vaultPda), isSigner: true, isWritable: false },
    ],
    data: Buffer.from(upgradeableLoaderUpgradeData),
  });
  const wrappedMessage = new Web3TransactionMessage({
    payerKey: toPublicKey(vaultPda),
    recentBlockhash: args.transactionBlockhash,
    instructions: [upgradeInstruction],
  });

  const createMessage = toLegacyMessageBytes({
    payerKey: args.member,
    recentBlockhash: args.transactionBlockhash,
    instructions: [
      sqdsInstructions.vaultTransactionCreate({
        multisigPda: toPublicKey(args.multisigPda),
        transactionIndex: args.transactionIndex,
        creator: toPublicKey(args.member),
        vaultIndex: args.vaultIndex,
        ephemeralSigners: 0,
        transactionMessage: wrappedMessage,
        programId: toPublicKey(SQUADS_PROGRAM_ADDRESS),
      }),
    ],
  });
  const proposalMessage = toLegacyMessageBytes({
    payerKey: args.member,
    recentBlockhash: args.proposalBlockhash,
    instructions: [
      sqdsInstructions.proposalCreate({
        multisigPda: toPublicKey(args.multisigPda),
        creator: toPublicKey(args.member),
        transactionIndex: args.transactionIndex,
        isDraft: false,
        programId: toPublicKey(SQUADS_PROGRAM_ADDRESS),
      }),
    ],
  });
  const create = buildHashedLegacyMessage(createMessage);
  const proposal = buildHashedLegacyMessage(proposalMessage);

  return {
    intentHash,
    vaultPda,
    transactionPda,
    proposalPda,
    programDataPda,
    create,
    proposal,
  };
}

export async function buildProposalExecuteUpgradeTransaction(args: {
  member: Address;
  multisigPda: Address;
  transactionIndex: bigint;
  vaultIndex: number;
  program: Address;
  buffer: Address;
  spill: Address;
  recentBlockhash: string;
  lastValidBlockHeight: bigint;
}): Promise<BuiltUpgradeExecuteTransaction> {
  const [vaultPda, transactionPda, proposalPda, programDataPda, intentHash] = await Promise.all([
    getVaultPda({ multisigPda: args.multisigPda, vaultIndex: args.vaultIndex }),
    getTransactionPda({ multisigPda: args.multisigPda, transactionIndex: args.transactionIndex }),
    getProposalPda({ multisigPda: args.multisigPda, transactionIndex: args.transactionIndex }),
    getProgramDataPda({ program: args.program }),
    buildUpgradeIntentHash({
      multisigPda: args.multisigPda,
      vaultIndex: args.vaultIndex,
      program: args.program,
      buffer: args.buffer,
      spill: args.spill,
    }),
  ]);

  const executeInstruction = sqdsGenerated.createVaultTransactionExecuteInstruction(
    {
      multisig: toPublicKey(args.multisigPda),
      proposal: toPublicKey(proposalPda),
      transaction: toPublicKey(transactionPda),
      member: toPublicKey(args.member),
      anchorRemainingAccounts: [
        { pubkey: toPublicKey(vaultPda), isSigner: false, isWritable: true },
        { pubkey: toPublicKey(programDataPda), isSigner: false, isWritable: true },
        { pubkey: toPublicKey(args.program), isSigner: false, isWritable: true },
        { pubkey: toPublicKey(args.buffer), isSigner: false, isWritable: true },
        { pubkey: toPublicKey(args.spill), isSigner: false, isWritable: true },
        {
          pubkey: toPublicKey(BPF_LOADER_UPGRADEABLE_PROGRAM_ADDRESS),
          isSigner: false,
          isWritable: false,
        },
        { pubkey: toPublicKey(SYSVAR_RENT_ADDRESS), isSigner: false, isWritable: false },
        { pubkey: toPublicKey(SYSVAR_CLOCK_ADDRESS), isSigner: false, isWritable: false },
      ],
    },
    toPublicKey(SQUADS_PROGRAM_ADDRESS),
  );
  const built = buildHashedLegacyMessage(
    toLegacyMessageBytes({
      payerKey: args.member,
      recentBlockhash: args.recentBlockhash,
      instructions: [executeInstruction],
    }),
  );

  return {
    intentHash,
    vaultPda,
    transactionPda,
    proposalPda,
    programDataPda,
    message: built.message,
    messageHash: built.messageHash,
  };
}

export function serializeLegacyMessageWithSignature(args: {
  message: Buffer;
  signature: Buffer;
}): { serializedTransaction: Buffer; base64EncodedWireTransaction: string } {
  const serializedTransaction = Buffer.concat([
    encodeShortvec(1),
    args.signature,
    args.message,
  ]);
  return {
    serializedTransaction,
    base64EncodedWireTransaction: serializedTransaction.toString("base64"),
  };
}

export async function sendSerializedTransaction(args: {
  connection: SolanaRpcClient;
  base64EncodedWireTransaction: string;
}): Promise<string> {
  return args.connection
    .sendTransaction(args.base64EncodedWireTransaction as never, {
      encoding: "base64",
      preflightCommitment: "confirmed",
    })
    .send();
}

export function decodeAddress(bytes: Uint8Array): Address {
  return addressDecoder.decode(bytes);
}

export function encodeAddressBytes(value: Address): Buffer {
  return Buffer.from(addressEncoder.encode(value));
}
