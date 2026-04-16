import { describe, expect, test } from "bun:test";

import { address } from "@solana/kit";

import { ProposalVote } from "../src/constants.js";
import {
  buildProposalCreateUpgradeTransactions,
  buildProposalExecuteUpgradeTransaction,
  buildProposalVoteTransaction,
} from "../src/squads.js";

describe("squads proposal vote transaction", () => {
  test("builds a stable legacy message", async () => {
    const member = address("11111111111111111111111111111112");
    const multisig = address("11111111111111111111111111111113");

    const built = await buildProposalVoteTransaction({
      member,
      feePayer: member,
      multisigPda: multisig,
      transactionIndex: 42n,
      vote: ProposalVote.Approve,
      recentBlockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 99n,
    });

    expect(built.message.toString("hex")).toContain("9025a488bcd82af800");
    expect(built.messageHash.toString("hex")).toHaveLength(64);
    expect(built.proposalPda).toBeTruthy();
  });

  test("builds a reject discriminator when requested", async () => {
    const member = address("11111111111111111111111111111112");
    const multisig = address("11111111111111111111111111111113");

    const built = await buildProposalVoteTransaction({
      member,
      feePayer: member,
      multisigPda: multisig,
      transactionIndex: 42n,
      vote: ProposalVote.Reject,
      recentBlockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 99n,
    });

    expect(built.message.toString("hex")).toContain("f33e869ce66af68700");
  });

  test("builds stable program-upgrade create transactions", async () => {
    const member = address("11111111111111111111111111111112");
    const multisig = address("11111111111111111111111111111113");
    const program = address("11111111111111111111111111111114");
    const buffer = address("11111111111111111111111111111115");
    const spill = address("11111111111111111111111111111116");

    const built = await buildProposalCreateUpgradeTransactions({
      member,
      multisigPda: multisig,
      transactionIndex: 42n,
      vaultIndex: 0,
      program,
      buffer,
      spill,
      transactionBlockhash: "11111111111111111111111111111111",
      proposalBlockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 99n,
    });

    expect(built.intentHash.toString("hex")).toHaveLength(64);
    expect(built.create.messageHash.toString("hex")).toHaveLength(64);
    expect(built.proposal.messageHash.toString("hex")).toHaveLength(64);
    expect(built.create.message.toString("hex")).toContain("30fa4ea8d0e2dad3");
    expect(built.proposal.message.toString("hex")).toContain("dc3c49e01e6c4f9f");
  });

  test("builds stable program-upgrade execute transactions", async () => {
    const member = address("11111111111111111111111111111112");
    const multisig = address("11111111111111111111111111111113");
    const program = address("11111111111111111111111111111114");
    const buffer = address("11111111111111111111111111111115");
    const spill = address("11111111111111111111111111111116");

    const built = await buildProposalExecuteUpgradeTransaction({
      member,
      multisigPda: multisig,
      transactionIndex: 42n,
      vaultIndex: 0,
      program,
      buffer,
      spill,
      recentBlockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 99n,
    });

    expect(built.intentHash.toString("hex")).toHaveLength(64);
    expect(built.messageHash.toString("hex")).toHaveLength(64);
    expect(built.message.toString("hex")).toContain("c208a15799a419ab");
  });
});
