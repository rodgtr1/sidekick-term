use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

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

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let socket = format!("{home}/.local/run/sidekick.sock");

    let mut stream = UnixStream::connect(&socket).unwrap_or_else(|e| {
        eprintln!("sidekick-ctl: could not connect to {socket}: {e}");
        eprintln!("Is sidekick running?");
        std::process::exit(1);
    });

    writeln!(stream, "{payload}").expect("write");

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).expect("read");
    print!("{response}");
}
