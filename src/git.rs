use std::process::Command;
use std::process::Stdio;

const MAX_GIT_STATUS_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Conflicted,
    Other,
}

impl GitStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            GitStatus::Modified => "M",
            GitStatus::Added => "A",
            GitStatus::Deleted => "D",
            GitStatus::Untracked => "?",
            GitStatus::Conflicted => "U",
            GitStatus::Other => "~",
        }
    }
    pub fn color(&self) -> &'static str {
        match self {
            GitStatus::Modified => "#f9e2af",
            GitStatus::Added => "#a6e3a1",
            GitStatus::Deleted => "#f38ba8",
            GitStatus::Untracked => "#89b4fa",
            GitStatus::Conflicted => "#fab387",
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

/// Unmerged entry from a conflicted merge/rebase/cherry-pick:
/// UU, AU, UA, DU, UD, AA or DD in porcelain output.
pub fn is_conflict_xy(x: char, y: char) -> bool {
    x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D')
}

pub fn ignored_set(root: &str) -> std::collections::HashSet<String> {
    let Ok(out) = Command::new("git")
        .args([
            "-C",
            root,
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

pub fn changed_files(root: &str) -> Vec<GitFile> {
    // -z gives NUL-delimited, unquoted paths (porcelain v1 C-quotes paths with
    // special characters otherwise, which would break abs_path).
    let out = match crate::limits::command_stdout_limited(
        Command::new("git").args(["-C", root, "status", "--porcelain=v1", "-z", "-u"]),
        MAX_GIT_STATUS_BYTES,
        &[],
        crate::limits::CapMode::Fail,
    ) {
        Ok(out) => out,
        Err(_) => return vec![],
    };
    parse_porcelain(&out, root)
}

fn parse_porcelain(out: &[u8], root: &str) -> Vec<GitFile> {
    let mut files = Vec::new();
    let mut records = out.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }
        let x = record[0] as char;
        let y = record[1] as char;
        let rel = String::from_utf8_lossy(&record[3..]).to_string();
        let rel = rel.as_str();
        // Renames/copies are followed by the origin path as a separate field.
        if x == 'R' || x == 'C' || y == 'R' || y == 'C' {
            let _origin = records.next();
        }

        if x == '?' && y == '?' {
            files.push(GitFile {
                rel_path: rel.to_string(),
                abs_path: format!("{}/{}", root, rel),
                status: GitStatus::Untracked,
                staged: false,
            });
            continue;
        }

        // A conflicted file isn't staged or unstaged in any useful sense —
        // one entry; the action that resolves it is Stage.
        if is_conflict_xy(x, y) {
            files.push(GitFile {
                rel_path: rel.to_string(),
                abs_path: format!("{}/{}", root, rel),
                status: GitStatus::Conflicted,
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
    let bytes = if file.staged {
        crate::limits::command_stdout_limited(
            Command::new("git").args([
                "-C",
                root,
                "diff",
                "--cached",
                "HEAD",
                "--",
                &file.rel_path,
            ]),
            crate::limits::MAX_DIFF_BYTES,
            &[],
            crate::limits::CapMode::Fail,
        )
        .or_else(|_| {
            // Repos without a HEAD yet (no commits): diff the index against
            // the empty tree so freshly staged files still preview.
            crate::limits::command_stdout_limited(
                Command::new("git").args(["-C", root, "diff", "--cached", "--", &file.rel_path]),
                crate::limits::MAX_DIFF_BYTES,
                &[],
                crate::limits::CapMode::Fail,
            )
        })?
    } else {
        crate::limits::command_stdout_limited(
            Command::new("git").args(["-C", root, "diff", "--", &file.rel_path]),
            crate::limits::MAX_DIFF_BYTES,
            &[],
            crate::limits::CapMode::Fail,
        )?
    };
    String::from_utf8(bytes).map_err(|_| "Diff is not valid UTF-8 text.".to_string())
}

pub fn current_branch(root: &str) -> Option<String> {
    Command::new("git")
        .args(["-C", root, "branch", "--show-current"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// (ahead, behind) of the current branch's upstream, from one
/// `git rev-list --left-right --count @{u}...HEAD` call. (0, 0) when no
/// upstream is configured (e.g. branch never pushed).
pub fn ahead_behind(root: &str) -> (u32, u32) {
    let out = Command::new("git")
        .args([
            "-C",
            root,
            "rev-list",
            "--left-right",
            "--count",
            "@{u}...HEAD",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => {
            parse_ahead_behind(&String::from_utf8_lossy(&o.stdout)).unwrap_or((0, 0))
        }
        _ => (0, 0),
    }
}

/// Parse rev-list --left-right --count output ("<behind>\t<ahead>") into
/// (ahead, behind).
fn parse_ahead_behind(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.trim().split('\t');
    let behind: u32 = parts.next()?.parse().ok()?;
    let ahead: u32 = parts.next()?.parse().ok()?;
    Some((ahead, behind))
}

pub fn stage(root: &str, rel_path: &str) -> Result<(), String> {
    run_git(root, &["add", "--", rel_path])
}

pub fn unstage(root: &str, rel_path: &str) -> Result<(), String> {
    run_git(root, &["restore", "--staged", "--", rel_path])
}

pub fn stage_all(root: &str) -> Result<(), String> {
    run_git(root, &["add", "-A"])
}

pub fn unstage_all(root: &str) -> Result<(), String> {
    run_git(root, &["reset", "HEAD", "--", "."]).or_else(|_| {
        // No HEAD yet (no commits): unstage by removing from the index.
        run_git(root, &["rm", "--cached", "-r", "--quiet", "--", "."])
    })
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

pub fn commit(root: &str, message: &str) -> Result<(), String> {
    if message.trim().is_empty() {
        return Err("Commit message cannot be empty.".to_string());
    }
    run_git(root, &["commit", "-m", message])
}

pub fn pull(cwd: &str) -> Result<(), String> {
    let root = repo_root(cwd).ok_or_else(|| "Not a git repository.".to_string())?;
    let out = Command::new("git")
        .args(["-C", &root, "pull"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
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
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ahead_behind_counts() {
        // git prints "<behind>\t<ahead>" (left = upstream-only commits).
        assert_eq!(parse_ahead_behind("2\t3\n"), Some((3, 2)));
        assert_eq!(parse_ahead_behind("0\t0"), Some((0, 0)));
    }

    #[test]
    fn rejects_malformed_ahead_behind() {
        assert_eq!(parse_ahead_behind(""), None);
        assert_eq!(parse_ahead_behind("nonsense"), None);
        assert_eq!(parse_ahead_behind("1"), None);
    }

    #[test]
    fn detects_conflict_status_codes() {
        for (x, y) in [
            ('U', 'U'),
            ('A', 'U'),
            ('U', 'A'),
            ('D', 'U'),
            ('U', 'D'),
            ('A', 'A'),
            ('D', 'D'),
        ] {
            assert!(is_conflict_xy(x, y), "{x}{y} should be a conflict");
        }
        assert!(!is_conflict_xy('M', 'M'));
        assert!(!is_conflict_xy('A', ' '));
        assert!(!is_conflict_xy('?', '?'));
        assert!(!is_conflict_xy(' ', 'D'));
    }

    #[test]
    fn porcelain_conflict_yields_single_unstaged_entry() {
        let files = parse_porcelain(b"UU both.txt\0", "/r");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, GitStatus::Conflicted);
        assert!(!files[0].staged);
        assert_eq!(files[0].rel_path, "both.txt");
        assert_eq!(files[0].abs_path, "/r/both.txt");
    }

    #[test]
    fn porcelain_partially_staged_still_yields_two_entries() {
        let files = parse_porcelain(b"MM file.txt\0", "/r");
        assert_eq!(files.len(), 2);
        assert!(files[0].staged);
        assert!(!files[1].staged);
    }
}
