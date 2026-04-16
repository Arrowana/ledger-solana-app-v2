#!/usr/bin/env bun

import { Socket } from "node:net";

type ScreenEvent = {
  text?: string;
  x?: number;
  y?: number;
  clear?: boolean;
};

type ScreenResponse = {
  events?: ScreenEvent[];
};

const apiPort = process.env.SPECULOS_API_PORT ?? "5001";
const apiBase = process.env.SPECULOS_API_URL ?? `http://127.0.0.1:${apiPort}`;
const automationPort = Number(process.env.SPECULOS_AUTOMATION_PORT ?? "41000");
const buttonPort = Number(process.env.SPECULOS_BUTTON_PORT ?? "42000");
const automationHost = process.env.SPECULOS_AUTOMATION_HOST ?? "127.0.0.1";
const buttonHost = process.env.SPECULOS_BUTTON_HOST ?? "127.0.0.1";
const [command = "screen"] = process.argv.slice(2);

switch (command) {
  case "screen":
    await printCurrentScreen();
    break;
  case "events":
    await streamEvents();
    break;
  case "left":
  case "right":
  case "both":
    await pressButton(command);
    await sleep(150);
    await printCurrentScreen();
    break;
  case "clear-events":
    await clearEvents();
    break;
  default:
    printUsage();
    process.exitCode = 1;
    break;
}

async function printCurrentScreen(): Promise<void> {
  try {
    const response = await fetchJson<ScreenResponse>(`${apiBase}/events?currentscreenonly=true`);
    const lines = normalizeTexts(response.events ?? []);

    console.log(`API: ${apiBase}`);
    if (lines.length === 0) {
      console.log("(no text events)");
      return;
    }

    for (const line of lines) {
      console.log(line);
    }
  } catch {
    const lines = await collectAutomationEvents(250);
    console.log(`Automation: ${automationHost}:${automationPort}`);
    if (lines.length === 0) {
      console.log("(no fresh text events; use `bun run speculos:events` while pressing buttons)");
      return;
    }
    for (const line of lines) {
      console.log(line);
    }
  }
}

async function streamEvents(): Promise<void> {
  try {
    const response = await fetch(`${apiBase}/events?stream=true`);
    if (!response.ok || !response.body) {
      throw new Error(`Failed to open event stream: ${response.status} ${response.statusText}`);
    }

    console.error(`Streaming events from ${apiBase}/events?stream=true`);
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }

      buffer += decoder.decode(value, { stream: true });
      for (;;) {
        const marker = buffer.indexOf("\n\n");
        if (marker === -1) {
          break;
        }

        const chunk = buffer.slice(0, marker);
        buffer = buffer.slice(marker + 2);

        for (const line of chunk.split("\n")) {
          if (!line.startsWith("data: ")) {
            continue;
          }
          const event = JSON.parse(line.slice(6)) as ScreenEvent;
          if (event.clear) {
            console.log("--- clear ---");
            continue;
          }
          if (event.text) {
            console.log(event.text);
          }
        }
      }
    }
  } catch {
    console.error(`Streaming events from ${automationHost}:${automationPort}`);
    await streamAutomationEvents();
  }
}

async function pressButton(button: "left" | "right" | "both"): Promise<void> {
  try {
    const response = await fetch(`${apiBase}/button/${button}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action: "press-and-release" }),
    });

    if (!response.ok) {
      throw new Error(`Failed to press ${button}: ${response.status} ${response.statusText}`);
    }
  } catch {
    await sendButtonSequence(button);
  }
}

async function clearEvents(): Promise<void> {
  const response = await fetch(`${apiBase}/events`, { method: "DELETE" });
  if (!response.ok) {
    throw new Error(`Failed to clear events: ${response.status} ${response.statusText}`);
  }
}

async function fetchJson<T>(url: string): Promise<T> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`Request failed: ${response.status} ${response.statusText}`);
  }
  return (await response.json()) as T;
}

function normalizeTexts(events: ScreenEvent[]): string[] {
  const seen = new Set<string>();
  const lines: string[] = [];

  for (const event of events) {
    const text = event.text?.trim();
    if (!text || seen.has(text)) {
      continue;
    }
    seen.add(text);
    lines.push(text);
  }

  return lines;
}

async function collectAutomationEvents(durationMs: number): Promise<string[]> {
  const socket = await connectSocket(automationHost, automationPort);
  const lines: string[] = [];
  let buffer = "";

  socket.on("data", (chunk: Buffer) => {
    buffer += chunk.toString("utf8");
    for (;;) {
      const marker = buffer.indexOf("\n");
      if (marker === -1) {
        break;
      }
      const line = buffer.slice(0, marker).trim();
      buffer = buffer.slice(marker + 1);
      if (!line) {
        continue;
      }
      const event = JSON.parse(line) as ScreenEvent;
      if (event.text) {
        lines.push(event.text);
      }
    }
  });

  await sleep(durationMs);
  socket.end();
  return dedupeLines(lines);
}

async function streamAutomationEvents(): Promise<void> {
  const socket = await connectSocket(automationHost, automationPort);
  let buffer = "";

  socket.on("data", (chunk: Buffer) => {
    buffer += chunk.toString("utf8");
    for (;;) {
      const marker = buffer.indexOf("\n");
      if (marker === -1) {
        break;
      }
      const line = buffer.slice(0, marker).trim();
      buffer = buffer.slice(marker + 1);
      if (!line) {
        continue;
      }
      const event = JSON.parse(line) as ScreenEvent;
      if (event.clear) {
        console.log("--- clear ---");
      } else if (event.text) {
        console.log(event.text);
      }
    }
  });

  await new Promise<void>(() => {});
}

async function sendButtonSequence(button: "left" | "right" | "both"): Promise<void> {
  const socket = await connectSocket(buttonHost, buttonPort);
  const sequence =
    button === "left" ? "Ll" : button === "right" ? "Rr" : "LRlr";
  socket.write(sequence);
  await sleep(150);
  socket.end();
}

function connectSocket(host: string, port: number): Promise<Socket> {
  return new Promise((resolve, reject) => {
    const socket = new Socket();
    socket.once("error", reject);
    socket.connect(port, host, () => {
      socket.removeListener("error", reject);
      resolve(socket);
    });
  });
}

function dedupeLines(lines: string[]): string[] {
  const seen = new Set<string>();
  const unique: string[] = [];
  for (const line of lines) {
    if (seen.has(line)) {
      continue;
    }
    seen.add(line);
    unique.push(line);
  }
  return unique;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function printUsage(): void {
  console.error(`Usage:
  bun scripts/speculos-ui.ts screen
  bun scripts/speculos-ui.ts events
  bun scripts/speculos-ui.ts left
  bun scripts/speculos-ui.ts right
  bun scripts/speculos-ui.ts both
  bun scripts/speculos-ui.ts clear-events`);
}
