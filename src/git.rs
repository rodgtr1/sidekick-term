use std::process::Command;

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
    let out = Command::new("git")
        .args(["-C", &root, "status", "--porcelain=v1", "-u"])
        .output()
        .ok()
        .filter(|o| o.status.success());
    let out = match out {
        Some(o) => o,
        None => return vec![],
    };
    let text = String::from_utf8_lossy(&out.stdout);
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

pub fn file_diff(root: &str, file: &GitFile) -> String {
    if file.status == GitStatus::Untracked {
        let content = std::fs::read_to_string(&file.abs_path).unwrap_or_default();
        let mut diff = format!(
            "--- /dev/null\n+++ b/{}\n@@ -0,0 +1,{} @@\n",
            file.rel_path,
            content.lines().count()
        );
        for line in content.lines() {
            diff.push('+');
            diff.push_str(line);
            diff.push('\n');
        }
        return diff;
    }
    // Staged: diff between HEAD and index. Unstaged: diff between index and working tree.
    let args: Vec<&str> = if file.staged {
        vec!["-C", root, "diff", "--cached", "HEAD", "--", &file.rel_path]
    } else {
        vec!["-C", root, "diff", "--", &file.rel_path]
    };
    Command::new("git")
        .args(&args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}
