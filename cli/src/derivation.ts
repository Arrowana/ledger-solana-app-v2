const HARDENED_OFFSET = 0x8000_0000;

export function parseDerivationPath(path: string): number[] {
  if (!path.startsWith("m/")) {
    throw new Error(`Invalid derivation path: ${path}`);
  }

  const parts = path
    .slice(2)
    .split("/")
    .filter((part) => part.length > 0);

  if (parts.length === 0 || parts.length > 5) {
    throw new Error(`Unsupported derivation path length: ${path}`);
  }

  const segments = parts.map((part) => {
    const hardened = part.endsWith("'");
    const valuePart = hardened ? part.slice(0, -1) : part;
    if (!/^\d+$/.test(valuePart)) {
      throw new Error(`Invalid derivation segment: ${part}`);
    }

    const value = Number.parseInt(valuePart, 10);
    if (!Number.isSafeInteger(value) || value < 0 || value >= HARDENED_OFFSET) {
      throw new Error(`Invalid derivation value: ${part}`);
    }

    return hardened ? (value | HARDENED_OFFSET) >>> 0 : value >>> 0;
  });

  if (
    segments[0] !== ((44 | HARDENED_OFFSET) >>> 0) ||
    segments[1] !== ((501 | HARDENED_OFFSET) >>> 0)
  ) {
    throw new Error("Only Solana derivation paths under m/44'/501' are supported");
  }

  return segments;
}

export function serializeDerivationPath(segments: readonly number[]): Buffer {
  if (segments.length === 0 || segments.length > 5) {
    throw new Error("Derivation path must contain 1-5 segments");
  }

  const out = Buffer.alloc(1 + segments.length * 4);
  out[0] = segments.length;
  for (const [index, segment] of segments.entries()) {
    out.writeUInt32BE(segment >>> 0, 1 + index * 4);
  }
  return out;
}

export function formatDerivationPath(segments: readonly number[]): string {
  const formatted = segments.map((segment) => {
    const hardened = (segment & HARDENED_OFFSET) !== 0;
    const value = segment & ~HARDENED_OFFSET;
    return hardened ? `${value}'` : `${value}`;
  });
  return `m/${formatted.join("/")}`;
}
