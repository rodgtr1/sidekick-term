use std::io::Read;
use std::path::Path;

pub const MAX_EDITOR_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DIRECTORY_ENTRIES: usize = 500;

pub fn read_text_file_limited(path: &str, max_bytes: u64) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Cannot inspect file: {e}"))?;
    if !meta.is_file() {
        return Err("Not a regular file".to_string());
    }
    if meta.len() > max_bytes {
        return Err(format!(
            "File is too large to preview safely ({} bytes, limit {} bytes).",
            meta.len(),
            max_bytes
        ));
    }

    let mut file = std::fs::File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read file: {e}"))?;

    if looks_binary(&bytes) {
        return Err("Binary file preview is not supported.".to_string());
    }

    String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8 text.".to_string())
}

pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

/// What to do when a child process produces more output than the limit.
#[derive(Clone, Copy, PartialEq)]
pub enum CapMode {
    /// Kill the child and return an error.
    Fail,
    /// Kill the child and return the output collected so far.
    Truncate,
}

/// Run a command and collect stdout, never holding more than `limit` bytes.
/// Exit codes listed in `ok_codes` are treated as success (e.g. grep/rg use 1
/// for "no matches").
pub fn command_stdout_limited(
    command: &mut std::process::Command,
    limit: usize,
    ok_codes: &[i32],
    mode: CapMode,
) -> Result<Vec<u8>, String> {
    use std::process::Stdio;

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Could not capture command output.".to_string())?;
    let mut output = Vec::new();
    let mut buf = [0u8; 8192];

    loop {
        let read = stdout.read(&mut buf).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&buf[..read]);
        if output.len() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return match mode {
                CapMode::Fail => Err("Command output is too large.".to_string()),
                CapMode::Truncate => {
                    output.truncate(limit);
                    Ok(output)
                }
            };
        }
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    let code = status.code().unwrap_or(-1);
    if status.success() || ok_codes.contains(&code) {
        Ok(output)
    } else {
        Err(format!("Command exited with {status}."))
    }
}

pub fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
