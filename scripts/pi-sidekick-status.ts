// Sidekick agent-status extension for the Pi coding agent.
//
// Reports Pi's lifecycle to Sidekick's agents panel using the same OSC 666
// termprop sequence that sidekick-agent-status emits for Claude Code/Codex
// hooks. Installed by scripts/install-agent-status-hooks.
//
// Mapping:
//   agent_start            -> busy  (working on a prompt)
//   agent_end              -> done  (back at the input prompt)
//   session_shutdown(quit) -> idle  (removed from the agents panel)
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { closeSync, openSync, writeSync } from "node:fs";

const TERMPROP = "vte.ext.sidekick.agent";

function report(status: "busy" | "ready" | "done" | "idle"): void {
  try {
    const fd = openSync("/dev/tty", "w");
    try {
      writeSync(fd, `\x1b]666;${TERMPROP}=${status}\x1b\\`);
    } finally {
      closeSync(fd);
    }
  } catch {
    // No controlling terminal (print/RPC mode) — nothing to report to.
  }
}

export default function (pi: ExtensionAPI) {
  pi.on("agent_start", async () => report("busy"));
  pi.on("agent_end", async () => report("done"));
  pi.on("session_shutdown", async (event) => {
    if (event.reason === "quit") {
      report("idle");
    }
  });
}
