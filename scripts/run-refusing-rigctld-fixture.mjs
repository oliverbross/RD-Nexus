#!/usr/bin/env node
import { appendFileSync } from "node:fs";
import { createServer } from "node:net";

const host = "127.0.0.1";
const port = Number(process.env.SHACKCQ_FAKE_RIG_PORT ?? 4601);
const transcript = process.env.SHACKCQ_FAKE_RIG_TRANSCRIPT;
const refuseFrequency = process.env.SHACKCQ_FAKE_RIG_REFUSE_FREQUENCY === "1";
const refuseMode = process.env.SHACKCQ_FAKE_RIG_REFUSE_MODE === "1";
let frequencyHz = Number(process.env.SHACKCQ_FAKE_RIG_FREQUENCY_HZ ?? 14_074_000);
let mode = process.env.SHACKCQ_FAKE_RIG_MODE ?? "USB";
let passbandHz = Number(process.env.SHACKCQ_FAKE_RIG_PASSBAND_HZ ?? 3_000);

if (!Number.isSafeInteger(port) || port < 1024 || port > 65535)
  throw new Error("SHACKCQ_FAKE_RIG_PORT_INVALID");
if (!Number.isSafeInteger(frequencyHz) || frequencyHz <= 0)
  throw new Error("SHACKCQ_FAKE_RIG_FREQUENCY_INVALID");
if (transcript && !transcript.startsWith("/"))
  throw new Error("SHACKCQ_FAKE_RIG_TRANSCRIPT_MUST_BE_ABSOLUTE");

function record(direction, line) {
  if (!transcript) return;
  appendFileSync(
    transcript,
    `${JSON.stringify({ at: new Date().toISOString(), direction, line })}\n`,
    { encoding: "utf8", mode: 0o600 },
  );
}

function reply(socket, value) {
  record("out", value.replace(/\n$/u, ""));
  socket.write(value);
}

const server = createServer((socket) => {
  socket.setEncoding("utf8");
  let buffered = "";
  socket.on("data", (chunk) => {
    buffered += chunk;
    for (;;) {
      const end = buffered.indexOf("\n");
      if (end < 0) break;
      const line = buffered.slice(0, end).replace(/\r$/u, "");
      buffered = buffered.slice(end + 1);
      record("in", line);
      if (line === "f") reply(socket, `${frequencyHz}\n`);
      else if (line === "m") reply(socket, `${mode}\n${passbandHz}\n`);
      else if (line === "t") reply(socket, "0\n");
      else if (line === "l STRENGTH") reply(socket, "-30\n");
      else if (line.startsWith("F ")) {
        const requested = Number(line.slice(2).trim());
        if (refuseFrequency || !Number.isSafeInteger(requested) || requested <= 0)
          reply(socket, "RPRT -1\n");
        else {
          frequencyHz = requested;
          reply(socket, "RPRT 0\n");
        }
      } else if (line.startsWith("M ")) {
        const [requestedMode, requestedPassband] = line.slice(2).trim().split(/\s+/u);
        if (refuseMode || !requestedMode) reply(socket, "RPRT -1\n");
        else {
          mode = requestedMode;
          const parsed = Number(requestedPassband);
          if (Number.isFinite(parsed) && parsed >= 0) passbandHz = parsed;
          reply(socket, "RPRT 0\n");
        }
      } else if (line === "q") socket.end();
      else reply(socket, "RPRT -11\n");
    }
  });
});

server.listen(port, host, () => {
  process.stdout.write(
    `${JSON.stringify({ event: "SHACKCQ_FAKE_RIG_READY", host, port, frequencyHz, mode, refuseFrequency, refuseMode, transcript: transcript ?? null })}\n`,
  );
});

for (const signal of ["SIGINT", "SIGTERM"])
  process.once(signal, () => server.close(() => process.exit(0)));
