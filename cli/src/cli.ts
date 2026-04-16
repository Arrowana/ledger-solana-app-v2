#!/usr/bin/env node

import { parseArgs } from "node:util";

import bs58 from "bs58";
import { address, type Address } from "@solana/kit";

import {
  buildListMultisigSlotApdu,
  buildProposalCreateUpgradeApdu,
  buildProposalExecuteUpgradeApdu,
  buildProposalVoteApdu,
  buildResetMultisigsApdu,
  buildSaveMultisigApdu,
  decodeApduResponse,
  decodeListMultisigSlotResponse,
  decodeProposalCreateUpgradeResponse,
  decodeProposalExecuteUpgradeResponse,
  decodeProposalVoteResponse,
  decodeSaveMultisigResponse,
} from "./apdu.js";
import {
  MAX_SAVED_MULTISIGS,
  ProposalVote,
  SW_OK,
  SW_USER_REFUSED,
  SW_NOT_FOUND,
  TransportKind,
} from "./constants.js";
import { formatDerivationPath, parseDerivationPath } from "./derivation.js";
import {
  buildProposalCreateUpgradeTransactions,
  buildProposalExecuteUpgradeTransaction,
  buildUpgradeIntentHash,
  buildProposalVoteTransaction,
  createConnection,
  decodeAddress,
  encodeAddressBytes,
  fetchValidatedMultisig,
  preflightProposalVote,
  serializeLegacyMessageWithSignature,
  sendSerializedTransaction,
} from "./squads.js";
import { openTransport } from "./transport.js";

type CliValues = {
  transport?: string;
  "rpc-url"?: string;
  multisig?: string;
  "derivation-path"?: string;
  "transaction-index"?: string;
  vote?: string;
  "fee-payer"?: string;
  program?: string;
  buffer?: string;
  spill?: string;
  "vault-index"?: string;
  blockhash?: string;
  "transaction-blockhash"?: string;
  "proposal-blockhash"?: string;
  send?: boolean;
  json?: boolean;
  "speculos-host"?: string;
  "speculos-port"?: string;
  "unsafe-non-confirm"?: boolean;
  "unsafe-skip-rpc-checks"?: boolean;
};

async function main(): Promise<void> {
  const { positionals, values } = parseArgs({
    allowPositionals: true,
    options: {
      transport: { type: "string" },
      "rpc-url": { type: "string" },
      multisig: { type: "string" },
      "derivation-path": { type: "string" },
      "transaction-index": { type: "string" },
      vote: { type: "string" },
      "fee-payer": { type: "string" },
      program: { type: "string" },
      buffer: { type: "string" },
      spill: { type: "string" },
      "vault-index": { type: "string" },
      blockhash: { type: "string" },
      "transaction-blockhash": { type: "string" },
      "proposal-blockhash": { type: "string" },
      send: { type: "boolean" },
      json: { type: "boolean" },
      "speculos-host": { type: "string" },
      "speculos-port": { type: "string" },
      "unsafe-non-confirm": { type: "boolean" },
      "unsafe-skip-rpc-checks": { type: "boolean" },
    },
  });

  const [command] = positionals;
  if (!command) {
    printUsage();
    process.exitCode = 1;
    return;
  }

  switch (command) {
    case "save-multisig":
      await handleSaveMultisig(values);
      return;
    case "list-saved":
      await handleListSaved(values);
      return;
    case "proposal-vote":
      await handleProposalVote(values);
      return;
    case "proposal-create-upgrade":
      await handleProposalCreateUpgrade(values);
      return;
    case "proposal-execute-upgrade":
      await handleProposalExecuteUpgrade(values);
      return;
    case "proposal-approve":
      await handleProposalVote({ ...values, vote: values.vote ?? "approve" });
      return;
    case "reset-multisigs":
      await handleReset(values);
      return;
    default:
      throw new Error(`Unknown command: ${command}`);
  }
}

async function handleSaveMultisig(rawValues: Record<string, string | boolean | undefined>) {
  const values = rawValues as CliValues;
  const multisigArg = requireString(values.multisig, "--multisig is required");
  const derivationPathArg = requireString(
    values["derivation-path"],
    "--derivation-path is required",
  );
  const multisig = address(multisigArg);
  const derivationPath = parseDerivationPath(derivationPathArg);
  const rpcUrl = values["rpc-url"];

  if (rpcUrl) {
    const connection = createConnection(rpcUrl);
    await fetchValidatedMultisig(connection, multisig);
  } else if (!skipRpcChecks(values)) {
    throw new Error("--rpc-url is required unless --unsafe-skip-rpc-checks is set");
  }

  const transport = await openTransportFromValues(values);
  try {
    const apdu = buildSaveMultisigApdu({
      multisig: encodeAddressBytes(multisig),
      derivationPath,
      nonConfirm: nonConfirm(values),
    });
    const response = decodeApduResponse(await transport.exchange(apdu));
    assertStatus(response.statusWord, "save multisig");
    const decoded = decodeSaveMultisigResponse(response.data);
    const output = {
      slot: decoded.slot,
      multisig,
      derivationPath: formatDerivationPath(derivationPath),
      member: bs58.encode(decoded.member),
    };
    printOutput(output, values.json ?? false);
  } finally {
    await transport.close();
  }
}

async function handleListSaved(rawValues: Record<string, string | boolean | undefined>) {
  const values = rawValues as CliValues;
  const transport = await openTransportFromValues(values);

  try {
    const entries = [];
    for (let slot = 0; slot < MAX_SAVED_MULTISIGS; slot += 1) {
      const response = decodeApduResponse(await transport.exchange(buildListMultisigSlotApdu(slot)));
      if (response.statusWord === SW_NOT_FOUND) {
        continue;
      }
      assertStatus(response.statusWord, `list slot ${slot}`);
      const entry = decodeListMultisigSlotResponse(slot, response.data);
      if (entry) {
        entries.push({
          slot: entry.slot,
          multisig: bs58.encode(entry.multisig),
          member: bs58.encode(entry.member),
          derivationPath: formatDerivationPath(entry.path),
        });
      }
    }

    printOutput({ entries }, values.json ?? false);
  } finally {
    await transport.close();
  }
}

async function handleProposalVote(rawValues: Record<string, string | boolean | undefined>) {
  const values = rawValues as CliValues;
  const multisigArg = requireString(values.multisig, "--multisig is required");
  const txIndexArg = requireString(values["transaction-index"], "--transaction-index is required");
  const vote = parseVote(values.vote);

  const multisig = address(multisigArg);
  const transactionIndex = BigInt(txIndexArg);
  const feePayer = values["fee-payer"] ? address(values["fee-payer"]) : undefined;
  const rpcUrl = values["rpc-url"];
  let connection = undefined;
  let recentBlockhash = values.blockhash;
  let lastValidBlockHeight = 0n;

  if (rpcUrl) {
    connection = createConnection(rpcUrl);
    const latestBlockhash = await connection.getLatestBlockhash({ commitment: "confirmed" }).send();
    recentBlockhash = values.blockhash ?? latestBlockhash.value.blockhash;
    lastValidBlockHeight = latestBlockhash.value.lastValidBlockHeight;
  } else if (!skipRpcChecks(values)) {
    throw new Error("--rpc-url is required unless --unsafe-skip-rpc-checks is set");
  }

  if (!recentBlockhash) {
    throw new Error("--blockhash is required when --rpc-url is omitted");
  }

  const transport = await openTransportFromValues(values);
  try {
    const entries = await loadSavedEntries(values, transport);
    const saved = entries.find((entry) => entry.multisig === multisig);
    if (!saved) {
      throw new Error(`Multisig is not saved on the Ledger: ${multisig}`);
    }

    const member = address(saved.member);
    if (feePayer && feePayer !== member) {
      throw new Error("Fee payer must equal the saved Ledger member signer in v1");
    }

    if (connection && !skipRpcChecks(values)) {
      await preflightProposalVote({
        connection,
        multisigPda: multisig,
        transactionIndex,
        member,
      });
    }

    const apdu = buildProposalVoteApdu({
      multisig: encodeAddressBytes(multisig),
      transactionIndex,
      vote,
      blockhash: Buffer.from(bs58.decode(recentBlockhash)),
      feePayer: feePayer ? encodeAddressBytes(feePayer) : undefined,
      nonConfirm: nonConfirm(values),
    });

    const response = decodeApduResponse(await transport.exchange(apdu));
    assertStatus(response.statusWord, "proposal vote");
    const signed = decodeProposalVoteResponse(response.data);

    const signedMember = decodeAddress(signed.member);
    const signedProposal = decodeAddress(signed.proposal);

    const built = await buildProposalVoteTransaction({
      member: signedMember,
      feePayer: feePayer ?? signedMember,
      multisigPda: multisig,
      transactionIndex,
      vote,
      recentBlockhash,
      lastValidBlockHeight,
    });

    if (built.proposalPda !== signedProposal) {
      throw new Error("Device-derived proposal PDA does not match host-derived PDA");
    }
    if (!built.messageHash.equals(signed.messageHash)) {
      throw new Error("Device message hash does not match host-computed message hash");
    }

    const signedTransaction = serializeLegacyMessageWithSignature({
      message: built.message,
      signature: signed.signature,
    });

    const result: Record<string, unknown> = {
      vote: vote === ProposalVote.Approve ? "approve" : "reject",
      multisig,
      transactionIndex: transactionIndex.toString(),
      member: signedMember,
      proposal: signedProposal,
      blockhash: recentBlockhash,
      messageHash: signed.messageHash.toString("hex"),
      signature: Buffer.from(signed.signature).toString("hex"),
      transactionBase64: signedTransaction.serializedTransaction.toString("base64"),
    };

    if (values.send) {
      if (!connection) {
        throw new Error("--send requires --rpc-url");
      }
      result.signatureBase58 = await sendSerializedTransaction({
        connection,
        base64EncodedWireTransaction: signedTransaction.base64EncodedWireTransaction,
      });
    }

    printOutput(result, values.json ?? false);
  } finally {
    await transport.close();
  }
}

async function handleProposalCreateUpgrade(rawValues: Record<string, string | boolean | undefined>) {
  const values = rawValues as CliValues;
  const multisig = address(requireString(values.multisig, "--multisig is required"));
  const transactionIndex = BigInt(
    requireString(values["transaction-index"], "--transaction-index is required"),
  );
  const vaultIndex = parseVaultIndex(values["vault-index"]);
  const program = address(requireString(values.program, "--program is required"));
  const bufferAccount = address(requireString(values.buffer, "--buffer is required"));
  const spill = address(requireString(values.spill, "--spill is required"));
  const rpcUrl = values["rpc-url"];
  let connection = undefined;
  let transactionBlockhash = values["transaction-blockhash"];
  let proposalBlockhash = values["proposal-blockhash"];
  let lastValidBlockHeight = 0n;

  if (rpcUrl) {
    connection = createConnection(rpcUrl);
    const latestBlockhash = await connection.getLatestBlockhash({ commitment: "confirmed" }).send();
    transactionBlockhash = transactionBlockhash ?? latestBlockhash.value.blockhash;
    proposalBlockhash = proposalBlockhash ?? latestBlockhash.value.blockhash;
    lastValidBlockHeight = latestBlockhash.value.lastValidBlockHeight;
    if (!skipRpcChecks(values)) {
      await fetchValidatedMultisig(connection, multisig);
    }
  } else if (!skipRpcChecks(values)) {
    throw new Error("--rpc-url is required unless --unsafe-skip-rpc-checks is set");
  }

  if (!transactionBlockhash || !proposalBlockhash) {
    throw new Error(
      "--transaction-blockhash and --proposal-blockhash are required when --rpc-url is omitted",
    );
  }

  const transport = await openTransportFromValues(values);
  try {
    const entries = await loadSavedEntries(values, transport);
    const saved = entries.find((entry) => entry.multisig === multisig);
    if (!saved) {
      throw new Error(`Multisig is not saved on the Ledger: ${multisig}`);
    }

    const member = address(saved.member);
    const apdu = buildProposalCreateUpgradeApdu({
      multisig: encodeAddressBytes(multisig),
      transactionIndex,
      vaultIndex,
      program: encodeAddressBytes(program),
      buffer: encodeAddressBytes(bufferAccount),
      spill: encodeAddressBytes(spill),
      transactionBlockhash: Buffer.from(bs58.decode(transactionBlockhash)),
      proposalBlockhash: Buffer.from(bs58.decode(proposalBlockhash)),
      nonConfirm: nonConfirm(values),
    });

    const response = decodeApduResponse(await transport.exchange(apdu));
    assertStatus(response.statusWord, "proposal create upgrade");
    const signed = decodeProposalCreateUpgradeResponse(response.data);

    const intentHash = await buildUpgradeIntentHash({
      multisigPda: multisig,
      vaultIndex,
      program,
      buffer: bufferAccount,
      spill,
    });
    if (!intentHash.equals(signed.intentHash)) {
      throw new Error("Device intent hash does not match host-computed intent hash");
    }

    const built = await buildProposalCreateUpgradeTransactions({
      member,
      multisigPda: multisig,
      transactionIndex,
      vaultIndex,
      program,
      buffer: bufferAccount,
      spill,
      transactionBlockhash,
      proposalBlockhash,
      lastValidBlockHeight,
    });

    if (!built.intentHash.equals(signed.intentHash)) {
      throw new Error("Host-built intent hash does not match device intent hash");
    }
    if (!built.create.messageHash.equals(signed.createMessageHash)) {
      throw new Error("Device create hash does not match host-computed hash");
    }
    if (!built.proposal.messageHash.equals(signed.proposalMessageHash)) {
      throw new Error("Device proposal hash does not match host-computed hash");
    }

    const signedCreate = serializeLegacyMessageWithSignature({
      message: built.create.message,
      signature: signed.createSignature,
    });
    const signedProposal = serializeLegacyMessageWithSignature({
      message: built.proposal.message,
      signature: signed.proposalSignature,
    });

    const result: Record<string, unknown> = {
      kind: "programUpgrade",
      multisig,
      transactionIndex: transactionIndex.toString(),
      vaultIndex,
      member,
      transactionPda: built.transactionPda,
      proposalPda: built.proposalPda,
      vaultPda: built.vaultPda,
      programData: built.programDataPda,
      program,
      buffer: bufferAccount,
      spill,
      intentHash: signed.intentHash.toString("hex"),
      create: {
        blockhash: transactionBlockhash,
        messageHash: signed.createMessageHash.toString("hex"),
        signature: Buffer.from(signed.createSignature).toString("hex"),
        transactionBase64: signedCreate.serializedTransaction.toString("base64"),
      },
      proposal: {
        blockhash: proposalBlockhash,
        messageHash: signed.proposalMessageHash.toString("hex"),
        signature: Buffer.from(signed.proposalSignature).toString("hex"),
        transactionBase64: signedProposal.serializedTransaction.toString("base64"),
      },
    };

    if (values.send) {
      if (!connection) {
        throw new Error("--send requires --rpc-url");
      }
      result.createSignatureBase58 = await sendSerializedTransaction({
        connection,
        base64EncodedWireTransaction: signedCreate.base64EncodedWireTransaction,
      });
      result.proposalSignatureBase58 = await sendSerializedTransaction({
        connection,
        base64EncodedWireTransaction: signedProposal.base64EncodedWireTransaction,
      });
    }

    printOutput(result, values.json ?? false);
  } finally {
    await transport.close();
  }
}

async function handleProposalExecuteUpgrade(rawValues: Record<string, string | boolean | undefined>) {
  const values = rawValues as CliValues;
  const multisig = address(requireString(values.multisig, "--multisig is required"));
  const transactionIndex = BigInt(
    requireString(values["transaction-index"], "--transaction-index is required"),
  );
  const vaultIndex = parseVaultIndex(values["vault-index"]);
  const program = address(requireString(values.program, "--program is required"));
  const bufferAccount = address(requireString(values.buffer, "--buffer is required"));
  const spill = address(requireString(values.spill, "--spill is required"));
  const rpcUrl = values["rpc-url"];
  let connection = undefined;
  let recentBlockhash = values.blockhash;
  let lastValidBlockHeight = 0n;

  if (rpcUrl) {
    connection = createConnection(rpcUrl);
    const latestBlockhash = await connection.getLatestBlockhash({ commitment: "confirmed" }).send();
    recentBlockhash = recentBlockhash ?? latestBlockhash.value.blockhash;
    lastValidBlockHeight = latestBlockhash.value.lastValidBlockHeight;
    if (!skipRpcChecks(values)) {
      await fetchValidatedMultisig(connection, multisig);
    }
  } else if (!skipRpcChecks(values)) {
    throw new Error("--rpc-url is required unless --unsafe-skip-rpc-checks is set");
  }

  if (!recentBlockhash) {
    throw new Error("--blockhash is required when --rpc-url is omitted");
  }

  const transport = await openTransportFromValues(values);
  try {
    const entries = await loadSavedEntries(values, transport);
    const saved = entries.find((entry) => entry.multisig === multisig);
    if (!saved) {
      throw new Error(`Multisig is not saved on the Ledger: ${multisig}`);
    }
    const member = address(saved.member);

    const apdu = buildProposalExecuteUpgradeApdu({
      multisig: encodeAddressBytes(multisig),
      transactionIndex,
      vaultIndex,
      program: encodeAddressBytes(program),
      buffer: encodeAddressBytes(bufferAccount),
      spill: encodeAddressBytes(spill),
      blockhash: Buffer.from(bs58.decode(recentBlockhash)),
      nonConfirm: nonConfirm(values),
    });

    const response = decodeApduResponse(await transport.exchange(apdu));
    assertStatus(response.statusWord, "proposal execute upgrade");
    const signed = decodeProposalExecuteUpgradeResponse(response.data);

    const built = await buildProposalExecuteUpgradeTransaction({
      member,
      multisigPda: multisig,
      transactionIndex,
      vaultIndex,
      program,
      buffer: bufferAccount,
      spill,
      recentBlockhash,
      lastValidBlockHeight,
    });

    if (!built.intentHash.equals(signed.intentHash)) {
      throw new Error("Device intent hash does not match host-computed intent hash");
    }
    if (!built.messageHash.equals(signed.messageHash)) {
      throw new Error("Device execute hash does not match host-computed hash");
    }

    const signedTransaction = serializeLegacyMessageWithSignature({
      message: built.message,
      signature: signed.signature,
    });

    const result: Record<string, unknown> = {
      kind: "programUpgrade",
      multisig,
      transactionIndex: transactionIndex.toString(),
      vaultIndex,
      member,
      transactionPda: built.transactionPda,
      proposalPda: built.proposalPda,
      vaultPda: built.vaultPda,
      programData: built.programDataPda,
      program,
      buffer: bufferAccount,
      spill,
      blockhash: recentBlockhash,
      intentHash: signed.intentHash.toString("hex"),
      messageHash: signed.messageHash.toString("hex"),
      signature: Buffer.from(signed.signature).toString("hex"),
      transactionBase64: signedTransaction.serializedTransaction.toString("base64"),
    };

    if (values.send) {
      if (!connection) {
        throw new Error("--send requires --rpc-url");
      }
      result.signatureBase58 = await sendSerializedTransaction({
        connection,
        base64EncodedWireTransaction: signedTransaction.base64EncodedWireTransaction,
      });
    }

    printOutput(result, values.json ?? false);
  } finally {
    await transport.close();
  }
}

async function handleReset(rawValues: Record<string, string | boolean | undefined>) {
  const values = rawValues as CliValues;
  const transport = await openTransportFromValues(values);
  try {
    const response = decodeApduResponse(
      await transport.exchange(buildResetMultisigsApdu(nonConfirm(values))),
    );
    assertStatus(response.statusWord, "reset multisigs");
    printOutput({ reset: true }, values.json ?? false);
  } finally {
    await transport.close();
  }
}

async function loadSavedEntries(values: CliValues, transport?: Awaited<ReturnType<typeof openTransport>>) {
  const client = transport ?? (await openTransportFromValues(values));
  const closeAfter = !transport;
  try {
    const entries = [];
    for (let slot = 0; slot < MAX_SAVED_MULTISIGS; slot += 1) {
      const response = decodeApduResponse(await client.exchange(buildListMultisigSlotApdu(slot)));
      if (response.statusWord === SW_NOT_FOUND) {
        continue;
      }
      assertStatus(response.statusWord, `list slot ${slot}`);
      const entry = decodeListMultisigSlotResponse(slot, response.data);
      if (entry) {
        entries.push({
          slot,
          multisig: bs58.encode(entry.multisig),
          member: bs58.encode(entry.member),
          derivationPath: formatDerivationPath(entry.path),
        });
      }
    }
    return entries;
  } finally {
    if (closeAfter) {
      await client.close();
    }
  }
}

function assertStatus(statusWord: number, label: string): void {
  if (statusWord === SW_OK) {
    return;
  }
  if (statusWord === SW_USER_REFUSED) {
    throw new Error(`Ledger user refused ${label}`);
  }
  throw new Error(`${label} failed with status 0x${statusWord.toString(16)}`);
}

function printUsage(): void {
  console.error(`Usage:
  save-multisig --transport hid|speculos [--rpc-url <url> | --unsafe-skip-rpc-checks] --multisig <pubkey> --derivation-path <path>
  list-saved --transport hid|speculos
  proposal-vote --transport hid|speculos [--rpc-url <url> | --unsafe-skip-rpc-checks] --multisig <pubkey> --transaction-index <u64> --vote approve|reject [--blockhash <hash>] [--fee-payer <pubkey>] [--send]
  proposal-create-upgrade --transport hid|speculos [--rpc-url <url> | --unsafe-skip-rpc-checks] --multisig <pubkey> --transaction-index <u64> --vault-index <u8> --program <pubkey> --buffer <pubkey> --spill <pubkey> [--transaction-blockhash <hash>] [--proposal-blockhash <hash>] [--send]
  proposal-execute-upgrade --transport hid|speculos [--rpc-url <url> | --unsafe-skip-rpc-checks] --multisig <pubkey> --transaction-index <u64> --vault-index <u8> --program <pubkey> --buffer <pubkey> --spill <pubkey> [--blockhash <hash>] [--send]
  reset-multisigs --transport hid|speculos`);
}

function requireString(value: string | undefined, error: string): string {
  if (!value) {
    throw new Error(error);
  }
  return value;
}

function nonConfirm(values: CliValues): boolean {
  return Boolean(values["unsafe-non-confirm"] || process.env.LEDGER_SQUADS_NON_CONFIRM === "1");
}

function skipRpcChecks(values: CliValues): boolean {
  return Boolean(values["unsafe-skip-rpc-checks"]);
}

function parseVote(value: string | undefined): ProposalVote {
  if (value === "approve") {
    return ProposalVote.Approve;
  }
  if (value === "reject") {
    return ProposalVote.Reject;
  }
  throw new Error("--vote must be either approve or reject");
}

function parseVaultIndex(value: string | undefined): number {
  const parsed = Number.parseInt(requireString(value, "--vault-index is required"), 10);
  if (!Number.isInteger(parsed) || parsed < 0 || parsed > 255) {
    throw new Error("--vault-index must be a u8");
  }
  return parsed;
}

async function openTransportFromValues(values: CliValues) {
  const kind = (values.transport ??
    process.env.LEDGER_SQUADS_TRANSPORT ??
    "hid") as TransportKind;
  const speculosPort = values["speculos-port"]
    ? Number.parseInt(values["speculos-port"], 10)
    : process.env.SPECULOS_APDU_PORT
      ? Number.parseInt(process.env.SPECULOS_APDU_PORT, 10)
      : undefined;

  return openTransport({
    kind,
    speculosHost: values["speculos-host"] ?? process.env.SPECULOS_HOST,
    speculosPort,
  });
}

function printOutput(payload: Record<string, unknown>, asJson: boolean): void {
  if (asJson) {
    console.log(JSON.stringify(payload, null, 2));
    return;
  }

  for (const [key, value] of Object.entries(payload)) {
    if (Array.isArray(value)) {
      console.log(`${key}:`);
      for (const entry of value) {
        console.log(`  ${JSON.stringify(entry)}`);
      }
      continue;
    }
    console.log(`${key}: ${value}`);
  }
}

await main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
