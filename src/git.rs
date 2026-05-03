use std::io::Read;
use std::process::Command;
use std::process::Stdio;

const MAX_GIT_STATUS_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Other,
}

impl GitStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            GitStatus::Modified => "M",
            GitStatus::Added => "A",
            GitStatus::Deleted => "D",
            GitStatus::Untracked => "?",
            GitStatus::Other => "~",
        }
    }
    pub fn color(&self) -> &'static str {
        match self {
            GitStatus::Modified => "#f9e2af",
            GitStatus::Added => "#a6e3a1",
            GitStatus::Deleted => "#f38ba8",
            GitStatus::Untracked => "#89b4fa",
            GitStatus::Other => "#6c7086",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitFile {
    pub rel_path: String,
    pub abs_path: String,
    pub status: GitStatus,
    pub staged: bool,
}

pub fn ignored_set(root: &str) -> std::collections::HashSet<String> {
    let Ok(out) = Command::new("git")
        .args([
            "-C", root,
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ])
        .output()
    else {
        return Default::default();
    };
    if !out.status.success() {
        return Default::default();
    }
    let mut set = std::collections::HashSet::new();
    for path in String::from_utf8_lossy(&out.stdout).split('\0') {
        if path.is_empty() {
            continue;
        }
        set.insert(format!("{}/{}", root, path.trim_end_matches('/')));
    }
    set
}

pub fn repo_root(cwd: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["-C", cwd, "rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn changed_files(cwd: &str) -> Vec<GitFile> {
    let root = match repo_root(cwd) {
        Some(r) => r,
        None => return vec![],
    };
    let out = match command_stdout_limited(
        Command::new("git").args(["-C", &root, "status", "--porcelain=v1", "-u"]),
        MAX_GIT_STATUS_BYTES,
    ) {
        Ok(out) => out,
        Err(_) => return vec![],
    };
    let text = String::from_utf8_lossy(&out);
    let mut files = Vec::new();
    for line in text.lines() {
        if line.len() < 4 {
            continue;
        }
        let xy = &line[..2];
        let rel = line[3..].trim();
        let rel = if rel.contains(" -> ") {
            rel.split(" -> ").last().unwrap_or(rel)
        } else {
            rel
        };
        let x = xy.chars().next().unwrap_or(' ');
        let y = xy.chars().nth(1).unwrap_or(' ');

        if x == '?' && y == '?' {
            files.push(GitFile {
                rel_path: rel.to_string(),
                abs_path: format!("{}/{}", root, rel),
                status: GitStatus::Untracked,
                staged: false,
            });
            continue;
        }

        // Staged entry (index column)
        if x != ' ' {
            let status = match x {
                'A' => GitStatus::Added,
                'D' => GitStatus::Deleted,
                'M' | 'R' => GitStatus::Modified,
                _ => GitStatus::Other,
            };
            files.push(GitFile {
                rel_path: rel.to_string(),
                abs_path: format!("{}/{}", root, rel),
                status,
                staged: true,
            });
        }

        // Unstaged entry (working tree column)
        if y != ' ' {
            let status = match y {
                'A' => GitStatus::Added,
                'D' => GitStatus::Deleted,
                'M' | 'R' => GitStatus::Modified,
                _ => GitStatus::Other,
            };
            files.push(GitFile {
                rel_path: rel.to_string(),
                abs_path: format!("{}/{}", root, rel),
                status,
                staged: false,
            });
        }
    }
    files
}

pub fn file_diff(root: &str, file: &GitFile) -> Result<String, String> {
    if file.status == GitStatus::Untracked {
        let content = crate::limits::read_text_file_limited(
            &file.abs_path,
            crate::limits::MAX_DIFF_BYTES as u64,
        )?;
        let mut diff = format!(
            "--- /dev/null\n+++ b/{}\n@@ -0,0 +1,{} @@\n",
            file.rel_path,
            content.lines().count()
        );
        for line in content.lines() {
            diff.push('+');
            diff.push_str(line);
            diff.push('\n');
            if diff.len() > crate::limits::MAX_DIFF_BYTES {
                return Err("Diff is too large to preview safely.".to_string());
            }
        }
        return Ok(diff);
    }
    // Staged: diff between HEAD and index. Unstaged: diff between index and working tree.
    let args: Vec<&str> = if file.staged {
        vec!["-C", root, "diff", "--cached", "HEAD", "--", &file.rel_path]
    } else {
        vec!["-C", root, "diff", "--", &file.rel_path]
    };
    let bytes = command_stdout_limited(
        Command::new("git").args(&args),
        crate::limits::MAX_DIFF_BYTES,
    )?;
    String::from_utf8(bytes).map_err(|_| "Diff is not valid UTF-8 text.".to_string())
}

pub fn ahead_count(cwd: &str) -> u32 {
    let root = match repo_root(cwd) {
        Some(r) => r,
        None => return 0,
    };
    let out = Command::new("git")
        .args(["-C", &root, "rev-list", "--count", "@{u}..HEAD"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
        }
        _ => 0,
    }
}

pub fn stage(root: &str, rel_path: &str) -> Result<(), String> {
    run_git(root, &["add", "--", rel_path])
}

pub fn unstage(root: &str, rel_path: &str) -> Result<(), String> {
    run_git(root, &["restore", "--staged", "--", rel_path])
}

pub fn discard(root: &str, rel_path: &str, is_untracked: bool) -> Result<(), String> {
    if is_untracked {
        run_git(root, &["clean", "-f", "--", rel_path])
    } else {
        run_git(root, &["restore", "--", rel_path])
    }
}

fn run_git(root: &str, args: &[&str]) -> Result<(), String> {
    let mut full_args = vec!["-C", root];
    full_args.extend_from_slice(args);
    let out = Command::new("git")
        .args(&full_args)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn push(cwd: &str) -> Result<(), String> {
    let root = repo_root(cwd).ok_or_else(|| "Not a git repository.".to_string())?;
    let out = Command::new("git")
        .args(["-C", &root, "push"])
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn command_stdout_limited(command: &mut Command, limit: usize) -> Result<Vec<u8>, String> {
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
            return Err("Command output is too large.".to_string());
        }
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(output)
    } else {
        Err(format!("Command exited with {status}."))
    }
}
