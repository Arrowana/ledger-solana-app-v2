import { describe, expect, test } from "bun:test";

import { ProposalVote } from "../src/constants.js";
import {
  buildProposalVoteApdu,
  buildSaveMultisigApdu,
  decodeListMultisigSlotResponse,
  decodeProposalVoteResponse,
  decodeSaveMultisigResponse,
} from "../src/apdu.js";

describe("apdu payloads", () => {
  test("encodes save multisig payload", () => {
    const multisig = Buffer.alloc(32, 0xaa);
    const apdu = buildSaveMultisigApdu({
      multisig,
      derivationPath: [0x8000002c, 0x800001f5, 0x80000000, 0x80000000],
      nonConfirm: false,
    });

    expect(apdu.subarray(0, 5).toString("hex")).toBe("e010000031");
    expect(apdu.length).toBe(54);
  });

  test("decodes list response", () => {
    const response = Buffer.concat([
      Buffer.from([1]),
      Buffer.alloc(32, 0x11),
      Buffer.alloc(32, 0x22),
      Buffer.from([4]),
      Buffer.from("8000002c800001f58000000080000000", "hex"),
    ]);

    const decoded = decodeListMultisigSlotResponse(3, response);
    expect(decoded?.slot).toBe(3);
    expect(decoded?.path.length).toBe(4);
  });

  test("decodes save response", () => {
    const response = Buffer.concat([Buffer.from([2]), Buffer.alloc(32, 0x33)]);
    const decoded = decodeSaveMultisigResponse(response);
    expect(decoded.slot).toBe(2);
    expect(decoded.member.length).toBe(32);
  });

  test("decodes proposal vote response", () => {
    const response = Buffer.concat([
      Buffer.alloc(64, 0x44),
      Buffer.alloc(32, 0x55),
      Buffer.alloc(32, 0x66),
      Buffer.alloc(32, 0x77),
    ]);

    const decoded = decodeProposalVoteResponse(response);
    expect(decoded.signature.length).toBe(64);
    expect(decoded.member.length).toBe(32);
    expect(decoded.proposal.length).toBe(32);
    expect(decoded.messageHash.length).toBe(32);
  });

  test("encodes proposal vote payload", () => {
    const apdu = buildProposalVoteApdu({
      multisig: Buffer.alloc(32, 0xaa),
      transactionIndex: 42n,
      vote: ProposalVote.Reject,
      blockhash: Buffer.alloc(32, 0xbb),
      feePayer: undefined,
      nonConfirm: true,
    });

    expect(apdu.subarray(0, 5).toString("hex")).toBe("e01201004a");
    expect(apdu[45]).toBe(ProposalVote.Reject);
  });
});
