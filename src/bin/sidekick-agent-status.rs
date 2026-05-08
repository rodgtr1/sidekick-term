use std::fs::OpenOptions;
use std::io::Write;

const TERMPROP: &str = "vte.ext.sidekick.agent";

fn main() {
    let status = match std::env::args().nth(1).as_deref() {
        Some("busy" | "working" | "running") => "busy",
        Some("ready" | "prompt" | "waiting" | "needs-user" | "needs_user") => "ready",
        Some("done" | "finished" | "complete") => "done",
        Some("idle" | "clear" | "reset") => "idle",
        _ => {
            eprintln!("usage: sidekick-agent-status busy|ready|done|idle");
            std::process::exit(2);
        }
    };

    let sequence = format!("\x1b]666;{TERMPROP}={status}\x1b\\");
    match OpenOptions::new().write(true).open("/dev/tty") {
        Ok(mut tty) => {
            let _ = tty.write_all(sequence.as_bytes());
            let _ = tty.flush();
        }
        Err(_) => {
            // Hooks often capture stdout/stderr. Avoid printing the escape
            // there, because it may be interpreted as hook output by the agent.
        }
    }
}
