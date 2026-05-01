use std::io::Read;
use std::path::Path;

pub const MAX_EDITOR_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DIRECTORY_ENTRIES: usize = 5_000;

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

pub fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
