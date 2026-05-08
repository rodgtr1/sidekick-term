use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let payload = match args.get(1).map(|s| s.as_str()) {
        Some("ping") => r#"{"action":"ping"}"#,
        Some("new-tab") => r#"{"action":"new_tab"}"#,
        Some("agent-busy") => r#"{"action":"agent_busy"}"#,
        Some("agent-ready") => r#"{"action":"agent_ready"}"#,
        Some("agent-done") => r#"{"action":"agent_done"}"#,
        Some("agent-idle") => r#"{"action":"agent_idle"}"#,
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
