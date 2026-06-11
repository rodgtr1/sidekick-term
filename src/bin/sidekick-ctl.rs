use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn socket_path() -> std::path::PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime.is_empty() {
            return std::path::PathBuf::from(runtime).join("sidekick/sidekick.sock");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".local/run/sidekick.sock")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Agent commands carry the tab id (injected into each shell's environment
    // by sidekick) so the status lands on the terminal the hook ran in, not
    // whichever tab happens to be focused.
    let tab = std::env::var("SIDEKICK_TAB_ID")
        .ok()
        .and_then(|v| v.parse::<u64>().ok());
    let agent_payload = |action: &str| match tab {
        Some(id) => format!(r#"{{"action":"{action}","tab":{id}}}"#),
        None => format!(r#"{{"action":"{action}"}}"#),
    };

    let payload = match args.get(1).map(|s| s.as_str()) {
        Some("ping") => r#"{"action":"ping"}"#.to_string(),
        Some("new-tab") => r#"{"action":"new_tab"}"#.to_string(),
        Some("agent-busy") => agent_payload("agent_busy"),
        Some("agent-ready") => agent_payload("agent_ready"),
        Some("agent-done") => agent_payload("agent_done"),
        Some("agent-idle") => agent_payload("agent_idle"),
        _ => {
            eprintln!(
                "Usage: sidekick-ctl <ping|new-tab|agent-busy|agent-ready|agent-done|agent-idle>"
            );
            std::process::exit(1);
        }
    };

    let socket = socket_path();
    let socket = socket.to_string_lossy().to_string();

    let mut stream = UnixStream::connect(&socket).unwrap_or_else(|e| {
        use std::io::ErrorKind;
        match e.kind() {
            // No socket file, or a stale socket with no listener (e.g. a
            // previous instance was killed) — sidekick simply isn't running.
            ErrorKind::NotFound | ErrorKind::ConnectionRefused => {
                eprintln!("sidekick-ctl: sidekick is not running.");
            }
            _ => {
                eprintln!("sidekick-ctl: could not connect to {socket}: {e}");
            }
        }
        std::process::exit(1);
    });

    writeln!(stream, "{payload}").expect("write");

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).expect("read");
    print!("{response}");
}
