use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;

const MAX_IPC_LINE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIFF_CONTENT_BYTES: usize = 4 * 1024 * 1024;

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
    AgentBusy {
        #[serde(default)]
        tab: Option<u64>,
    },
    AgentReady {
        #[serde(default)]
        tab: Option<u64>,
    },
    AgentDone {
        #[serde(default)]
        tab: Option<u64>,
    },
    AgentIdle {
        #[serde(default)]
        tab: Option<u64>,
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
    // Prefer the per-user runtime dir (0700, tmpfs). Fall back to the historic
    // ~/.local/run location when XDG_RUNTIME_DIR is unset.
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime.is_empty() {
            return std::path::PathBuf::from(runtime).join("sidekick/sidekick.sock");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".local/run/sidekick.sock")
}

/// Start the socket server. Returns a channel receiver that delivers commands
/// to whatever async task the caller attaches on the GTK main thread.
pub fn start() -> async_channel::Receiver<Request> {
    let path = socket_path();

    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("sidekick: socket dir create failed: {e}");
        }
        if let Err(e) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
            eprintln!("sidekick: socket dir chmod failed: {e}");
        }
    }

    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_socket() => {
            let _ = std::fs::remove_file(&path);
        }
        Ok(_) => {
            eprintln!(
                "sidekick: refusing to remove non-socket path {}",
                path.display()
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            eprintln!("sidekick: socket metadata failed: {e}");
        }
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
        if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!("sidekick: socket chmod failed: {e}");
        }
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || handle(stream, tx));
        }
    });

    rx
}

fn handle(stream: std::os::unix::net::UnixStream, sender: async_channel::Sender<Request>) {
    let mut writer = stream.try_clone().expect("clone socket");
    if !peer_is_current_user(&stream) {
        let resp = Response {
            ok: false,
            error: Some("peer uid does not match current user".to_string()),
            accepted: None,
        };
        write_response(&mut writer, &resp);
        return;
    }

    let mut reader = BufReader::new(stream);

    loop {
        let mut bytes = Vec::new();
        let read = match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if read > MAX_IPC_LINE_BYTES || bytes.len() > MAX_IPC_LINE_BYTES {
            let resp = Response {
                ok: false,
                error: Some("IPC request is too large".to_string()),
                accepted: None,
            };
            write_response(&mut writer, &resp);
            break;
        }

        let line = match String::from_utf8(bytes) {
            Ok(l) if !l.trim().is_empty() => l,
            Ok(_) => continue,
            Err(e) => {
                let resp = Response {
                    ok: false,
                    error: Some(e.to_string()),
                    accepted: None,
                };
                write_response(&mut writer, &resp);
                continue;
            }
        };

        let command: Command = match serde_json::from_str(&line) {
            Ok(c) => c,
            Err(e) => {
                let r = Response {
                    ok: false,
                    error: Some(e.to_string()),
                    accepted: None,
                };
                write_response(&mut writer, &r);
                continue;
            }
        };
        if let Err(error) = validate_command(&command) {
            let resp = Response {
                ok: false,
                error: Some(error),
                accepted: None,
            };
            write_response(&mut writer, &resp);
            continue;
        }

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
                write_response(&mut writer, &resp);
            }
            Err(_) => break,
        }
    }
}

fn validate_command(command: &Command) -> Result<(), String> {
    match command {
        Command::ShowDiff {
            old, new_content, ..
        } => {
            if old.len() > MAX_DIFF_CONTENT_BYTES || new_content.len() > MAX_DIFF_CONTENT_BYTES {
                Err("diff content is too large".to_string())
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn write_response(writer: &mut std::os::unix::net::UnixStream, resp: &Response) {
    if let Ok(json) = serde_json::to_string(resp) {
        let _ = writeln!(writer, "{json}");
    }
}

#[cfg(target_os = "linux")]
fn peer_is_current_user(stream: &std::os::unix::net::UnixStream) -> bool {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    rc == 0 && cred.uid == unsafe { libc::geteuid() }
}

#[cfg(target_os = "macos")]
fn peer_is_current_user(stream: &std::os::unix::net::UnixStream) -> bool {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    rc == 0 && uid == unsafe { libc::geteuid() }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peer_is_current_user(_stream: &std::os::unix::net::UnixStream) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_commands_parse_with_and_without_tab() {
        let c: Command = serde_json::from_str(r#"{"action":"agent_busy","tab":7}"#).unwrap();
        assert!(matches!(c, Command::AgentBusy { tab: Some(7) }));

        // Older clients omit the tab field entirely.
        let c: Command = serde_json::from_str(r#"{"action":"agent_ready"}"#).unwrap();
        assert!(matches!(c, Command::AgentReady { tab: None }));
    }

    #[test]
    fn show_diff_size_limit_enforced() {
        let big = "x".repeat(MAX_DIFF_CONTENT_BYTES + 1);
        let cmd = Command::ShowDiff {
            path: "/tmp/f".into(),
            old: big,
            new_content: String::new(),
        };
        assert!(validate_command(&cmd).is_err());
    }
}
