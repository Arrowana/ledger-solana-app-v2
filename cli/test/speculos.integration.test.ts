import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { fileURLToPath } from "node:url";

import { address } from "@solana/kit";

import {
  buildListMultisigSlotApdu,
  buildResetMultisigsApdu,
  decodeApduResponse,
} from "../src/apdu.js";
import { SW_OK } from "../src/constants.js";
import { getProposalPda } from "../src/squads.js";
import { openTransport } from "../src/transport.js";

const shouldRun = process.env.LEDGER_SQUADS_TRANSPORT === "speculos";
const speculosHost = process.env.SPECULOS_HOST ?? "127.0.0.1";
const speculosPort = process.env.SPECULOS_APDU_PORT
  ? Number.parseInt(process.env.SPECULOS_APDU_PORT, 10)
  : 9999;
const multisigAddress = address("11111111111111111111111111111113");
const derivationPath = "m/44'/501'/0'/0'";
const proposalTransactionIndex = 42n;
const recentBlockhash = "11111111111111111111111111111111";
const upgradeProgram = address("11111111111111111111111111111114");
const upgradeBuffer = address("11111111111111111111111111111115");
const upgradeSpill = address("11111111111111111111111111111116");

beforeEach(async () => {
  await resetDevice();
});

afterEach(async () => {
  await resetDevice();
});

describe.if(shouldRun)("speculos transport", () => {
  test("responds to list-slot requests once the app is loaded", async () => {
    const transport = await openTransport({
      kind: "speculos",
      speculosHost,
      speculosPort,
    });

    try {
      const response = decodeApduResponse(await transport.exchange(buildListMultisigSlotApdu(0)));
      expect([SW_OK, 0x6a88]).toContain(response.statusWord);
    } finally {
      await transport.close();
    }
  });

  test("saves a fictive multisig through the CLI", async () => {
    const saved = await runCliJson<{
      slot: number;
      multisig: string;
      derivationPath: string;
      member: string;
    }>([
      "save-multisig",
      "--transport",
      "speculos",
      "--speculos-host",
      speculosHost,
      "--speculos-port",
      String(speculosPort),
      "--multisig",
      multisigAddress,
      "--derivation-path",
      derivationPath,
      "--unsafe-skip-rpc-checks",
      "--unsafe-non-confirm",
      "--json",
    ]);

    expect(saved.slot).toBe(0);
    expect(saved.multisig).toBe(multisigAddress);
    expect(saved.derivationPath).toBe(derivationPath);
    expect(saved.member).toHaveLength(44);

    const listed = await runCliJson<{
      entries: Array<{
        slot: number;
        multisig: string;
        member: string;
        derivationPath: string;
      }>;
    }>([
      "list-saved",
      "--transport",
      "speculos",
      "--speculos-host",
      speculosHost,
      "--speculos-port",
      String(speculosPort),
      "--json",
    ]);

    expect(listed.entries).toEqual([
      {
        slot: 0,
        multisig: multisigAddress,
        member: saved.member,
        derivationPath,
      },
    ]);
  });

  test("builds a proposal vote through the CLI without RPC", async () => {
    const saved = await runCliJson<{ member: string }>([
      "save-multisig",
      "--transport",
      "speculos",
      "--speculos-host",
      speculosHost,
      "--speculos-port",
      String(speculosPort),
      "--multisig",
      multisigAddress,
      "--derivation-path",
      derivationPath,
      "--unsafe-skip-rpc-checks",
      "--unsafe-non-confirm",
      "--json",
    ]);

    const proposalAddress = await getProposalPda({
      multisigPda: multisigAddress,
      transactionIndex: proposalTransactionIndex,
    });

    const voted = await runCliJson<{
      vote: string;
      multisig: string;
      transactionIndex: string;
      member: string;
      proposal: string;
      blockhash: string;
      messageHash: string;
      signature: string;
      transactionBase64: string;
    }>([
      "proposal-vote",
      "--transport",
      "speculos",
      "--speculos-host",
      speculosHost,
      "--speculos-port",
      String(speculosPort),
      "--multisig",
      multisigAddress,
      "--transaction-index",
      proposalTransactionIndex.toString(),
      "--vote",
      "approve",
      "--blockhash",
      recentBlockhash,
      "--unsafe-skip-rpc-checks",
      "--unsafe-non-confirm",
      "--json",
    ]);

    expect(voted.vote).toBe("approve");
    expect(voted.multisig).toBe(multisigAddress);
    expect(voted.transactionIndex).toBe(proposalTransactionIndex.toString());
    expect(voted.member).toBe(saved.member);
    expect(voted.proposal).toBe(proposalAddress);
    expect(voted.blockhash).toBe(recentBlockhash);
    expect(voted.messageHash).toHaveLength(64);
    expect(voted.signature).toHaveLength(128);
    expect(voted.transactionBase64.length).toBeGreaterThan(0);
  });

  test("creates and executes a program upgrade proposal through the CLI without RPC", async () => {
    const saved = await runCliJson<{ member: string }>([
      "save-multisig",
      "--transport",
      "speculos",
      "--speculos-host",
      speculosHost,
      "--speculos-port",
      String(speculosPort),
      "--multisig",
      multisigAddress,
      "--derivation-path",
      derivationPath,
      "--unsafe-skip-rpc-checks",
      "--unsafe-non-confirm",
      "--json",
    ]);

    const created = await runCliJson<{
      member: string;
      transactionPda: string;
      proposalPda: string;
      vaultPda: string;
      programData: string;
      intentHash: string;
      create: { messageHash: string; transactionBase64: string };
      proposal: { messageHash: string; transactionBase64: string };
    }>([
      "proposal-create-upgrade",
      "--transport",
      "speculos",
      "--speculos-host",
      speculosHost,
      "--speculos-port",
      String(speculosPort),
      "--multisig",
      multisigAddress,
      "--transaction-index",
      proposalTransactionIndex.toString(),
      "--vault-index",
      "0",
      "--program",
      upgradeProgram,
      "--buffer",
      upgradeBuffer,
      "--spill",
      upgradeSpill,
      "--transaction-blockhash",
      recentBlockhash,
      "--proposal-blockhash",
      recentBlockhash,
      "--unsafe-skip-rpc-checks",
      "--unsafe-non-confirm",
      "--json",
    ]);

    expect(created.member).toBe(saved.member);
    expect(created.intentHash).toHaveLength(64);
    expect(created.create.messageHash).toHaveLength(64);
    expect(created.proposal.messageHash).toHaveLength(64);
    expect(created.create.transactionBase64.length).toBeGreaterThan(0);
    expect(created.proposal.transactionBase64.length).toBeGreaterThan(0);

    const executed = await runCliJson<{
      member: string;
      transactionPda: string;
      proposalPda: string;
      vaultPda: string;
      programData: string;
      intentHash: string;
      messageHash: string;
      transactionBase64: string;
    }>([
      "proposal-execute-upgrade",
      "--transport",
      "speculos",
      "--speculos-host",
      speculosHost,
      "--speculos-port",
      String(speculosPort),
      "--multisig",
      multisigAddress,
      "--transaction-index",
      proposalTransactionIndex.toString(),
      "--vault-index",
      "0",
      "--program",
      upgradeProgram,
      "--buffer",
      upgradeBuffer,
      "--spill",
      upgradeSpill,
      "--blockhash",
      recentBlockhash,
      "--unsafe-skip-rpc-checks",
      "--unsafe-non-confirm",
      "--json",
    ]);

    expect(executed.member).toBe(saved.member);
    expect(executed.transactionPda).toBe(created.transactionPda);
    expect(executed.proposalPda).toBe(created.proposalPda);
    expect(executed.vaultPda).toBe(created.vaultPda);
    expect(executed.programData).toBe(created.programData);
    expect(executed.intentHash).toBe(created.intentHash);
    expect(executed.messageHash).toHaveLength(64);
    expect(executed.transactionBase64.length).toBeGreaterThan(0);
  });
});

async function resetDevice(): Promise<void> {
  const transport = await openTransport({
    kind: "speculos",
    speculosHost,
    speculosPort,
  });

  try {
    const response = decodeApduResponse(await transport.exchange(buildResetMultisigsApdu(true)));
    expect(response.statusWord).toBe(SW_OK);
  } finally {
    await transport.close();
  }
}

async function runCliJson<T>(args: string[]): Promise<T> {
  const cliRoot = fileURLToPath(new URL("../", import.meta.url));
  const proc = Bun.spawn({
    cmd: ["bun", "src/cli.ts", ...args],
    cwd: cliRoot,
    stdout: "pipe",
    stderr: "pipe",
    env: {
      ...process.env,
      LEDGER_SQUADS_NON_CONFIRM: "1",
    },
  });

  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);

  if (exitCode !== 0) {
    throw new Error(`CLI failed with code ${exitCode}: ${stderr || stdout}`);
  }

  return JSON.parse(stdout) as T;
}
