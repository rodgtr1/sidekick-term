use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;

#[derive(Deserialize, Debug)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Command {
    Ping,
    NewTab,
    ShowDiff {
        path: String,
        old: String,
        #[serde(rename = "new")]
        new_content: String,
    },
}

#[derive(Serialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
}

pub struct Request {
    pub command: Command,
    pub reply: mpsc::SyncSender<Response>,
}

pub fn socket_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".local/run/sidekick.sock")
}

/// Start the socket server. Returns a channel receiver that delivers commands
/// to whatever async task the caller attaches on the GTK main thread.
pub fn start() -> async_channel::Receiver<Request> {
    let path = socket_path();

    let _ = std::fs::remove_file(&path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    let (tx, rx) = async_channel::unbounded::<Request>();

    std::thread::spawn(move || {
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("sidekick: socket bind failed: {e}");
                return;
            }
        };
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let tx = tx.clone();
                std::thread::spawn(move || handle(stream, tx));
            }
        }
    });

    rx
}

fn handle(stream: std::os::unix::net::UnixStream, sender: async_channel::Sender<Request>) {
    let mut writer = stream.try_clone().expect("clone socket");
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => break,
        };

        let command: Command = match serde_json::from_str(&line) {
            Ok(c) => c,
            Err(e) => {
                let r = Response {
                    ok: false,
                    error: Some(e.to_string()),
                    accepted: None,
                };
                let _ = writeln!(writer, "{}", serde_json::to_string(&r).unwrap());
                continue;
            }
        };

        let (reply_tx, reply_rx) = mpsc::sync_channel(0);
        if sender
            .send_blocking(Request {
                command,
                reply: reply_tx,
            })
            .is_err()
        {
            break;
        }

        match reply_rx.recv() {
            Ok(resp) => {
                let _ = writeln!(writer, "{}", serde_json::to_string(&resp).unwrap());
            }
            Err(_) => break,
        }
    }
}
