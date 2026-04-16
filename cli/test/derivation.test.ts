import { describe, expect, test } from "bun:test";

import {
  formatDerivationPath,
  parseDerivationPath,
  serializeDerivationPath,
} from "../src/derivation.js";

describe("derivation path handling", () => {
  test("round-trips a standard Solana path", () => {
    const segments = parseDerivationPath("m/44'/501'/0'/0'");
    expect(formatDerivationPath(segments)).toBe("m/44'/501'/0'/0'");

    const encoded = serializeDerivationPath(segments);
    expect(encoded.toString("hex")).toBe("048000002c800001f58000000080000000");
  });

  test("rejects non-Solana roots", () => {
    expect(() => parseDerivationPath("m/44'/0'/0'/0'")).toThrow(
      "Only Solana derivation paths under m/44'/501' are supported",
    );
  });
});

