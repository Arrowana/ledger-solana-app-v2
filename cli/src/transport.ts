import net from "node:net";

import type { TransportKind } from "./constants.js";

export interface DeviceTransport {
  exchange(apdu: Buffer): Promise<Buffer>;
  close(): Promise<void>;
}

class SpeculosTransport implements DeviceTransport {
  private readonly host: string;
  private readonly port: number;
  private socket: net.Socket | null = null;

  public constructor(host: string, port: number) {
    this.host = host;
    this.port = port;
  }

  private async connect(): Promise<net.Socket> {
    if (this.socket) {
      return this.socket;
    }

    const socket = await new Promise<net.Socket>((resolve, reject) => {
      const candidate = net.createConnection({ host: this.host, port: this.port }, () => {
        resolve(candidate);
      });
      candidate.once("error", reject);
    });

    this.socket = socket;
    return socket;
  }

  public async exchange(apdu: Buffer): Promise<Buffer> {
    const socket = await this.connect();
    const length = Buffer.alloc(4);
    length.writeUInt32BE(apdu.length, 0);

    await new Promise<void>((resolve, reject) => {
      socket.write(Buffer.concat([length, apdu]), (error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve();
      });
    });

    const responseLength = await readExactly(socket, 4);
    const payload = await readExactly(socket, responseLength.readUInt32BE(0));
    const statusWord = await readExactly(socket, 2);
    return Buffer.concat([payload, statusWord]);
  }

  public async close(): Promise<void> {
    if (!this.socket) {
      return;
    }

    await new Promise<void>((resolve) => {
      this.socket?.end(() => resolve());
    });
    this.socket = null;
  }
}

async function readExactly(socket: net.Socket, length: number): Promise<Buffer> {
  const chunks: Buffer[] = [];
  let received = 0;

  while (received < length) {
    const chunk = await new Promise<Buffer>((resolve, reject) => {
      const onData = (data: Buffer) => {
        cleanup();
        resolve(data);
      };
      const onError = (error: Error) => {
        cleanup();
        reject(error);
      };
      const onClose = () => {
        cleanup();
        reject(new Error("Socket closed while waiting for Speculos response"));
      };
      const cleanup = () => {
        socket.off("data", onData);
        socket.off("error", onError);
        socket.off("close", onClose);
      };

      socket.once("data", onData);
      socket.once("error", onError);
      socket.once("close", onClose);
    });

    chunks.push(chunk);
    received += chunk.length;
  }

  const combined = Buffer.concat(chunks);
  const wanted = combined.subarray(0, length);
  const extra = combined.subarray(length);
  if (extra.length > 0) {
    socket.unshift(extra);
  }
  return wanted;
}

export async function openTransport(args: {
  kind: TransportKind;
  speculosHost?: string;
  speculosPort?: number;
}): Promise<DeviceTransport> {
  if (args.kind === "speculos") {
    return new SpeculosTransport(args.speculosHost ?? "127.0.0.1", args.speculosPort ?? 9999);
  }

  const mod = await import("@ledgerhq/hw-transport-node-hid");
  const TransportNodeHid = (mod as any).default ?? (mod as any).TransportNodeHid ?? mod;
  const instance =
    typeof TransportNodeHid.open === "function"
      ? await TransportNodeHid.open()
      : await TransportNodeHid.create();

  return {
    async exchange(apdu: Buffer): Promise<Buffer> {
      const response = await instance.exchange(apdu);
      return Buffer.from(response);
    },
    async close(): Promise<void> {
      if (typeof instance.close === "function") {
        await instance.close();
      }
    },
  };
}

