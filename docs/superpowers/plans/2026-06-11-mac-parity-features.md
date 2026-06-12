# Mac Parity Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the five post-parity macOS sidekick features to the Linux GTK app: git pull/behind counts, merge-conflict handling in the git panel, a Ctrl+Shift+J attention-jump shortcut, agent-hook installer improvements (PreToolUse/SessionEnd hooks, sidekick-hook auto-wiring, Pi support), and Teleport opt-in + Beam node support.

**Architecture:** Sidekick is a single-binary GTK4/VTE Rust app (`src/main.rs` ~3700 lines) plus helper bins and focused modules. Each feature is a surgical edit to existing modules. Pure logic (porcelain parsing, conflict classification, jump-target selection, tsh JSON parsing, ahead/behind parsing) gets extracted into testable functions with unit tests; GTK glue gets manual-verification steps. The installer is a bash script with inline Python — verified by running it against a sandbox `$HOME`.

**Tech Stack:** Rust, gtk4-rs, vte4, serde_json, toml. Tests via `cargo test`. The reference implementation is the macOS app at `~/Repos/sidekick-term-mac` (commits `29aac8f`, `c3023d2`, `5f1ea5e`).

---

### Task 1: Git ahead/behind counts (pull number)

Replace `git::ahead_count` with a single `git rev-list --left-right --count @{u}...HEAD` call returning both counts; show the behind count on the pull button the same way the push button shows ahead.

**Files:**
- Modify: `src/git.rs:225-238` (replace `ahead_count`)
- Modify: `src/gitpanel.rs:119-125` (add `update_pull_button`)
- Modify: `src/main.rs:104-110` (UiResult::Git), `src/main.rs:528-551` (refresh_git), `src/main.rs:613-640` (Git handler), `src/main.rs:648-662` (Push handler), `src/main.rs:752-773` (5s poll)
- Test: `src/git.rs` (new `#[cfg(test)] mod tests` at end of file)

- [ ] **Step 1: Write the failing tests**

`src/git.rs` currently has no test module. Add at the end of the file:

```rust
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
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin sidekick parses_ahead_behind`
Expected: FAIL to compile — `parse_ahead_behind` not found.

- [ ] **Step 3: Implement `ahead_behind` in git.rs**

Replace the whole `ahead_count` function (`src/git.rs:225-238`) with:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass (main.rs won't compile yet — that's next)**

Run: `cargo test --bin sidekick parse`
Expected: compile error in main.rs (`ahead_count` not found). Proceed to step 5.

- [ ] **Step 5: Add `update_pull_button` to gitpanel.rs**

After `update_push_button` (`src/gitpanel.rs:119-125`) add:

```rust
pub fn update_pull_button(btn: &gtk4::Button, behind: u32) {
    if behind == 0 {
        btn.set_label("↓  pull");
    } else {
        btn.set_label(&format!("↓  pull  {behind}"));
    }
}
```

- [ ] **Step 6: Thread `behind` through main.rs**

Four edits:

1. `UiResult::Git` variant (`src/main.rs:104-110`) — add a field:

```rust
    Git {
        cwd: String,
        root: String,
        files: Vec<git::GitFile>,
        ahead: u32,
        behind: u32,
        branch: Option<String>,
    },
```

2. `refresh_git` producer (`src/main.rs:537-549`) — replace `let ahead = git::ahead_count(&root);` with `let (ahead, behind) = git::ahead_behind(&root);` and add `behind,` to the `UiResult::Git { ... }` literal.

3. 5-second poll producer (`src/main.rs:757-769`) — replace `let ahead = git::ahead_count(&cwd);` with `let (ahead, behind) = git::ahead_behind(&root);` (note: use `&root`, which is already computed on the line above — the old `&cwd` arg worked but `root` is the known repo root) and add `behind,` to the literal.

4. Consumer (`src/main.rs:613-640`) — add `behind,` to the destructuring pattern, and after the `gitpanel::update_push_button(&push_btn_c, ahead);` line add:

```rust
                            gitpanel::update_pull_button(&pull_btn_c, behind);
```

Also in the `UiResult::Push` success arm (`src/main.rs:649-652`), add `refresh_git_c();` after `push_btn_c.set_sensitive(true);` so the ahead count clears right after a successful push (the Pull arm already does this).

- [ ] **Step 7: Run tests and clippy**

Run: `cargo test --bin sidekick && cargo clippy --all-targets -- -D warnings`
Expected: all tests pass (including the two new ones), no clippy errors.

- [ ] **Step 8: Commit**

```bash
git add src/git.rs src/gitpanel.rs src/main.rs
git commit -m "feat: show behind count on pull button (single ahead/behind git call)"
```

---

### Task 2: Detect merge conflicts in git status

Porcelain `UU`/`AU`/`UA`/`DU`/`UD`/`AA`/`DD` entries become a single `Conflicted` file (peach `U` badge) listed in a CONFLICTS section at the top of the git panel, with a "Mark resolved (stage)" context-menu action instead of Stage/Discard.

**Files:**
- Modify: `src/git.rs` (GitStatus variant, `is_conflict_xy`, extract `parse_porcelain` from `changed_files`)
- Modify: `src/gitpanel.rs:132-174` (populate sections), `src/gitpanel.rs:190-256` (add_file_row), `src/gitpanel.rs:269-351` (context menu)
- Test: `src/git.rs` tests module (from Task 1)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/git.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin sidekick porcelain`
Expected: FAIL to compile — `is_conflict_xy` / `parse_porcelain` not found.

- [ ] **Step 3: Implement in git.rs**

1. Add a variant to `GitStatus` (`src/git.rs:6-13`):

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Conflicted,
    Other,
}
```

2. Extend `symbol()` and `color()` (`src/git.rs:16-33`):

```rust
            GitStatus::Conflicted => "U",
```
(in `symbol`, before the `Other` arm), and

```rust
            GitStatus::Conflicted => "#fab387",
```
(in `color`, before the `Other` arm — peach, distinct from Modified's yellow).

3. Add the classifier near the top of the file (after the `GitFile` struct):

```rust
/// Unmerged entry from a conflicted merge/rebase/cherry-pick:
/// UU, AU, UA, DU, UD, AA or DD in porcelain output.
pub fn is_conflict_xy(x: char, y: char) -> bool {
    x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D')
}
```

4. Split `changed_files` (`src/git.rs:84-154`): keep the command invocation, move the parse loop into a new function, and add the conflict branch. The result:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin sidekick`
Expected: PASS (all, including the three new tests).

- [ ] **Step 5: Show a CONFLICTS section in the git panel**

In `src/gitpanel.rs` `populate` (`src/gitpanel.rs:132-174`), replace the staged/unstaged partition block (lines 155-171) with:

```rust
    let conflicted: Vec<_> = files
        .iter()
        .filter(|f| f.status == git::GitStatus::Conflicted)
        .collect();
    let staged: Vec<_> = files
        .iter()
        .filter(|f| f.staged && f.status != git::GitStatus::Conflicted)
        .collect();
    let unstaged: Vec<_> = files
        .iter()
        .filter(|f| !f.staged && f.status != git::GitStatus::Conflicted)
        .collect();
    let staged_count = staged.len();

    if !conflicted.is_empty() {
        add_section_header(list, "CONFLICTS");
        for file in &conflicted {
            add_file_row(list, file, false, root, on_refresh);
        }
    }

    if !staged.is_empty() {
        add_section_header(list, "STAGED");
        for file in &staged {
            add_file_row(list, file, true, root, on_refresh);
        }
    }

    if !unstaged.is_empty() {
        add_section_header(list, "UNSTAGED");
        for file in &unstaged {
            add_file_row(list, file, false, root, on_refresh);
        }
    }

    staged_count
```

- [ ] **Step 6: Context menu — "Mark resolved (stage)" for conflicts**

In `add_file_row` (`src/gitpanel.rs:190-256`), next to the existing `is_untracked` local add:

```rust
    let is_conflicted = file.status == git::GitStatus::Conflicted;
```

and pass it to `show_context_menu` (add `is_conflicted,` after the `is_untracked` argument in the `gesture.connect_pressed` closure call).

In `show_context_menu` (`src/gitpanel.rs:269-351`), add the parameter `is_conflicted: bool` after `is_untracked: bool`, and change the menu construction: the `if staged { ... } else { ... }` becomes:

```rust
    if is_conflicted {
        // Staging an unmerged path tells git the conflict is resolved.
        add_menu_item(&vbox, "Mark resolved (stage)", &popover, {
            let path = rel_path.to_string();
            let root = root.to_string();
            let refresh = Rc::clone(on_refresh);
            let parent_w = parent.clone();
            move || match git::stage(&root, &path) {
                Ok(()) => refresh(),
                Err(e) => show_git_error(&parent_w, &e),
            }
        });
    } else if staged {
        // ... existing Unstage item unchanged ...
    } else {
        // ... existing Stage + Discard items unchanged ...
    }
```

(No Discard for conflicts — `git restore` on an unmerged path errors, and silently nuking a half-merged file is a footgun.)

- [ ] **Step 7: Build, test, clippy**

Run: `cargo test --bin sidekick && cargo clippy --all-targets -- -D warnings`
Expected: PASS / clean.

- [ ] **Step 8: Manual verification (do this; it's quick)**

```bash
d=$(mktemp -d) && cd $d && git init -q . && git commit -q --allow-empty -m init
echo base > f.txt && git add f.txt && git commit -q -m base
git checkout -q -b other && echo theirs > f.txt && git commit -qam theirs
git checkout -q - && echo ours > f.txt && git commit -qam ours
git merge other 2>/dev/null; git status --short   # expect: UU f.txt
```

Launch the worktree build (`cargo run` from the worktree, or `target/debug/sidekick`), `cd` to `$d` in the terminal, open the git panel: `f.txt` should appear under **CONFLICTS** with a peach `U`. Right-click → only "Mark resolved (stage)". Click it → file moves to STAGED.

- [ ] **Step 9: Commit**

```bash
git add src/git.rs src/gitpanel.rs
git commit -m "feat: surface merge conflicts in git panel with mark-resolved action"
```

---

### Task 3: Conflict viewer (highlighted conflict markers)

Clicking a conflicted file can't show a normal diff (git refuses to diff unmerged paths), so open the working-tree contents with `<<<<<<<`/`|||||||`/`=======`/`>>>>>>>` markers highlighted and the ours/base/theirs sections tinted.

**Files:**
- Modify: `src/git.rs` (add `conflict_file_content`)
- Modify: `src/diff.rs` (add `ConflictSection`, `conflict_line_tag`, `open_conflict`)
- Modify: `src/main.rs:111-114` (UiResult), `src/main.rs:642-647` (handler), `src/main.rs:775-806` (row activation)
- Test: `src/diff.rs:332-350` tests module

- [ ] **Step 1: Write the failing tests**

Add to the existing `tests` module in `src/diff.rs`:

```rust
    use super::{conflict_line_tag, ConflictSection};

    #[test]
    fn classifies_conflict_sections_in_order() {
        use ConflictSection::*;
        let lines = [
            ("plain text", "plain", None),
            ("<<<<<<< HEAD", "marker", Ours),
            ("our line", "ours", Ours),
            ("||||||| base", "marker", Base),
            ("base line", "base", Base),
            ("=======", "marker", Theirs),
            ("their line", "theirs", Theirs),
            (">>>>>>> other", "marker", None),
            ("after", "plain", None),
        ];
        let mut section = ConflictSection::None;
        for (line, want_tag, want_section) in lines {
            let (tag, next) = conflict_line_tag(line, section);
            assert_eq!(tag, want_tag, "line: {line}");
            assert_eq!(next, want_section, "line: {line}");
            section = next;
        }
    }

    #[test]
    fn stray_separators_outside_conflicts_are_plain() {
        // A bare ======= in ordinary content (e.g. a Markdown underline)
        // must not start a section.
        let (tag, next) = conflict_line_tag("=======", ConflictSection::None);
        assert_eq!(tag, "plain");
        assert_eq!(next, ConflictSection::None);
        let (tag, _) = conflict_line_tag(">>>>>>> x", ConflictSection::None);
        assert_eq!(tag, "plain");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin sidekick conflict`
Expected: FAIL to compile — `conflict_line_tag` not found.

- [ ] **Step 3: Implement the classifier and viewer in diff.rs**

Add after `open_readonly`:

```rust
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ConflictSection {
    None,
    Ours,
    Base,
    Theirs,
}

/// Tag for one line of a conflicted file plus the section the next line is
/// in. Markers are only honored in the order git writes them, so separator
/// lines in ordinary content (a Markdown `=======` underline, say) read as
/// plain text.
pub fn conflict_line_tag(
    line: &str,
    section: ConflictSection,
) -> (&'static str, ConflictSection) {
    use ConflictSection::*;
    if line.starts_with("<<<<<<<") {
        return ("marker", Ours);
    }
    if line.starts_with("|||||||") && section == Ours {
        return ("marker", Base);
    }
    if line.starts_with("=======") && (section == Ours || section == Base) {
        return ("marker", Theirs);
    }
    if line.starts_with(">>>>>>>") && section == Theirs {
        return ("marker", None);
    }
    let tag = match section {
        None => "plain",
        Ours => "ours",
        Base => "base",
        Theirs => "theirs",
    };
    (tag, section)
}

/// Open a read-only view of a conflicted file's working-tree contents with
/// the conflict markers highlighted and ours/base/theirs sections tinted.
pub fn open_conflict(title: &str, content: &str, notebook: &gtk4::Notebook) {
    if content.len() > crate::limits::MAX_DIFF_BYTES {
        open_message(
            "conflict too large",
            title,
            "File is too large to preview safely.",
            notebook,
        );
        return;
    }

    let buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);
    let tag_marker = buffer.create_tag(Some("marker"), &[]).unwrap();
    tag_marker.set_property("foreground", "#f9e2af");
    tag_marker.set_property("background", "#3a3326");
    let tag_ours = buffer.create_tag(Some("ours"), &[]).unwrap();
    tag_ours.set_property("background", "#16322f");
    let tag_base = buffer.create_tag(Some("base"), &[]).unwrap();
    tag_base.set_property("background", "#2a2b36");
    let tag_theirs = buffer.create_tag(Some("theirs"), &[]).unwrap();
    tag_theirs.set_property("background", "#1e2f4d");

    let mut text = String::new();
    let mut spans: Vec<(usize, usize, &'static str)> = Vec::new();
    let mut section = ConflictSection::None;
    for line in content.lines() {
        let (tag, next) = conflict_line_tag(line, section);
        section = next;
        let start = text.len();
        text.push_str(line);
        text.push('\n');
        if tag != "plain" {
            spans.push((start, text.len(), tag));
        }
    }

    buffer.set_text(&text);
    let mut boundaries = Vec::with_capacity(spans.len() * 2);
    for (s, e, _) in &spans {
        boundaries.push(*s);
        boundaries.push(*e);
    }
    let chars = char_offsets(&text, &boundaries);
    for (i, (_, _, tag)) in spans.iter().enumerate() {
        let si = buffer.iter_at_offset(chars[i * 2] as i32);
        let ei = buffer.iter_at_offset(chars[i * 2 + 1] as i32);
        buffer.apply_tag_by_name(tag, &si, &ei);
    }

    let view = gtk4::TextView::with_buffer(&buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.add_css_class("editor-view");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&view));
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);

    let tab_label = gtk4::Label::new(Some(&format!("⚠ {}", title)));
    let page_idx = notebook.n_pages();
    notebook.append_page(&scroll, Some(&tab_label));
    notebook.set_tab_reorderable(&scroll, true);
    notebook.set_current_page(Some(page_idx));
    view.grab_focus();
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin sidekick conflict`
Expected: PASS (both new tests).

- [ ] **Step 5: Add `conflict_file_content` to git.rs**

After `file_diff` (`src/git.rs`, around line 212):

```rust
/// Working-tree contents of a conflicted file. Unmerged paths can't be
/// diffed against the index (`git diff` emits combined-diff format), so the
/// viewer shows the raw file with its conflict markers instead.
pub fn conflict_file_content(root: &str, rel_path: &str) -> Result<String, String> {
    crate::limits::read_text_file_limited(
        &format!("{root}/{rel_path}"),
        crate::limits::MAX_DIFF_BYTES as u64,
    )
}
```

- [ ] **Step 6: Wire it up in main.rs**

1. Add a `UiResult` variant after `Diff` (`src/main.rs:111-114`):

```rust
    Conflict {
        title: String,
        result: Result<String, String>,
    },
```

2. Handler — after the `UiResult::Diff` arm (`src/main.rs:642-647`):

```rust
                    UiResult::Conflict { title, result } => match result {
                        Ok(content) => diff::open_conflict(&title, &content, &nb_c),
                        Err(message) => {
                            diff::open_message("conflict unavailable", &title, &message, &nb_c)
                        }
                    },
```

3. Row activation (`src/main.rs:775-806`) — inside the `if let Some(file) = ...` body, branch on status. Replace the existing `std::thread::spawn` block with:

```rust
                let cwd = last_cwd_c.borrow().clone();
                let file = file.clone();
                let title = file.rel_path.clone();
                let tx = tx.clone();
                if file.status == git::GitStatus::Conflicted {
                    std::thread::spawn(move || {
                        let result = git::repo_root(&cwd)
                            .ok_or_else(|| "Not inside a git repository.".to_string())
                            .and_then(|root| git::conflict_file_content(&root, &file.rel_path));
                        let _ = tx.send_blocking(UiResult::Conflict { title, result });
                    });
                } else {
                    std::thread::spawn(move || {
                        let result = git::repo_root(&cwd)
                            .ok_or_else(|| "Not inside a git repository.".to_string())
                            .and_then(|root| git::file_diff(&root, &file));
                        let _ = tx.send_blocking(UiResult::Diff { title, result });
                    });
                }
```

- [ ] **Step 7: Build, test, clippy**

Run: `cargo test --bin sidekick && cargo clippy --all-targets -- -D warnings`
Expected: PASS / clean.

- [ ] **Step 8: Manual verification**

Reuse the conflicted repo from Task 2 step 8. Click `f.txt` in the CONFLICTS section: a `⚠ f.txt` tab opens showing the file with `<<<<<<< HEAD` / `=======` / `>>>>>>> other` lines in yellow-on-dark-yellow and the two sections tinted teal/blue.

- [ ] **Step 9: Commit**

```bash
git add src/git.rs src/diff.rs src/main.rs
git commit -m "feat: conflict viewer tab with highlighted ours/base/theirs sections"
```

---

### Task 4: Ctrl+Shift+J — jump to next agent needing attention

Cycles through tabs by urgency: waiting-for-input first, then done, then running. Repeated presses walk all tabs in the most urgent non-empty state.

**Files:**
- Modify: `src/main.rs` (pure helper + key handler arm at `src/main.rs:1333`, new tests module at end of file)
- Modify: `src/shortcutshelp.rs:7-13` (Tabs section)
- Test: `src/main.rs` (new `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

`src/main.rs` has no tests module; add at the very end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_jump_prefers_waiting_tabs() {
        let pages = [
            (0, AgentState::Done),
            (1, AgentState::Ready),
            (2, AgentState::Busy),
            (3, AgentState::Ready),
        ];
        // From tab 0, the first Ready tab after it wins — not the Done tab.
        assert_eq!(attention_jump_target(&pages, 0), Some(1));
        // Repeated presses walk all Ready tabs, wrapping around.
        assert_eq!(attention_jump_target(&pages, 1), Some(3));
        assert_eq!(attention_jump_target(&pages, 3), Some(1));
    }

    #[test]
    fn attention_jump_falls_back_to_done_then_running() {
        let pages = [(0, AgentState::Idle), (1, AgentState::Done)];
        assert_eq!(attention_jump_target(&pages, 0), Some(1));
        let pages = [(0, AgentState::Idle), (1, AgentState::AutoBusy)];
        assert_eq!(attention_jump_target(&pages, 0), Some(1));
    }

    #[test]
    fn attention_jump_none_when_nothing_to_do() {
        assert_eq!(attention_jump_target(&[], 0), None);
        let pages = [(0, AgentState::Idle), (1, AgentState::Idle)];
        assert_eq!(attention_jump_target(&pages, 0), None);
        // Only candidate is the current tab: stay put.
        let pages = [(0, AgentState::Ready), (1, AgentState::Idle)];
        assert_eq!(attention_jump_target(&pages, 0), None);
    }
}
```

Note: the test matches on `AgentState` variants which already derive `Clone, Copy, PartialEq` (`src/main.rs:46`) — no derive changes needed.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin sidekick attention_jump`
Expected: FAIL to compile — `attention_jump_target` not found.

- [ ] **Step 3: Implement the helper**

Add next to `tab_jump_index` (`src/main.rs:3191`):

```rust
/// Ctrl+Shift+J: the page to jump to among `pages` (page index, agent
/// state), most urgent state first — waiting for input, then done, then
/// running. Repeated presses walk all tabs in the chosen state. None when
/// no tab wants attention or the only candidate is the current page.
fn attention_jump_target(pages: &[(u32, AgentState)], current: u32) -> Option<u32> {
    let groups: [&[AgentState]; 3] = [
        &[AgentState::Ready],
        &[AgentState::Done],
        &[AgentState::Busy, AgentState::AutoBusy],
    ];
    for wanted in groups {
        let mut candidates: Vec<u32> = pages
            .iter()
            .filter(|(_, s)| wanted.contains(s))
            .map(|(i, _)| *i)
            .collect();
        if candidates.is_empty() {
            continue;
        }
        candidates.sort_unstable();
        let next = candidates
            .iter()
            .copied()
            .find(|i| *i > current)
            .unwrap_or(candidates[0]);
        return (next != current).then_some(next);
    }
    None
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --bin sidekick attention_jump`
Expected: PASS (3 tests).

- [ ] **Step 5: Add the key-handler arm**

In the keyboard match (`src/main.rs:1163-1343`), before the `// Jump to tab N (Ctrl+1 .. Ctrl+9)` arm, add:

```rust
                // Jump to the next tab whose agent wants attention
                (true, true, false, gdk::Key::j | gdk::Key::J) => {
                    let mut pages: Vec<(u32, AgentState)> = Vec::new();
                    for i in 0..nb.n_pages() {
                        let Some(page) = nb.nth_page(Some(i)) else {
                            continue;
                        };
                        let Some(term) =
                            pane::collect_terminals_pub(&page).into_iter().next()
                        else {
                            continue;
                        };
                        let key = term.as_ptr() as usize;
                        if let Some((_, cell)) = agent_map_kb.borrow().get(&key) {
                            pages.push((i, cell.get()));
                        }
                    }
                    let current = nb.current_page().unwrap_or(0);
                    if let Some(target) = attention_jump_target(&pages, current) {
                        nb.set_current_page(Some(target));
                    }
                    glib::Propagation::Stop
                }
```

(`agent_map_kb` and `nb` are already captured by this closure; `pane::collect_terminals_pub` is the same lookup the agents dashboard uses at `src/main.rs:1016`.)

- [ ] **Step 6: Document the shortcut**

`src/shortcutshelp.rs` Tabs section — after the `("Ctrl+1 … Ctrl+9", ...)` line add:

```rust
            ("Ctrl+Shift+J", "Jump to next agent needing attention"),
```

- [ ] **Step 7: Build, test, clippy**

Run: `cargo test --bin sidekick && cargo clippy --all-targets -- -D warnings`
Expected: PASS / clean.

- [ ] **Step 8: Manual verification**

Run the app, open two tabs, run `sidekick-agent-status ready` in tab 2 while focused on tab 1 (or just `~/.local/bin/sidekick-agent-status ready` if installed; otherwise `printf '\033]666;vte.ext.sidekick.agent=ready\033\\'`). Press Ctrl+Shift+J from tab 1 → lands on tab 2. Ctrl+Shift+? shows the new entry.

- [ ] **Step 9: Commit**

```bash
git add src/main.rs src/shortcutshelp.rs
git commit -m "feat: Ctrl+Shift+J jumps to the next agent needing attention"
```

---

### Task 5: Teleport opt-in config + Beam node support

`[hosts] show_teleport = false` gates the TELEPORT section (tsh is never invoked when off; live-reloads). Beam instances (label `teleport.internal/beams/alias`) list under their alias and connect via `tsh beams ssh <alias>` instead of an undialable `tsh ssh beam-<uuid>`.

**Files:**
- Modify: `src/config.rs` (HostsConfig + default template + test)
- Modify: `src/hostspanel.rs` (show_teleport cell, `parse_teleport_nodes`, tests)
- Modify: `src/main.rs:213` (build arg), `src/main.rs:833-862` (reload_config)
- Test: `src/config.rs:211-225` and `src/hostspanel.rs:233-265` tests modules

- [ ] **Step 1: Write the failing config test**

Add to the `tests` module in `src/config.rs`:

```rust
    #[test]
    fn hosts_show_teleport_parses_and_defaults_off() {
        let cfg = parse_config("[hosts]\nshow_teleport = true\n").expect("valid");
        assert!(cfg.hosts.show_teleport);
        assert!(!parse_config("").expect("valid").hosts.show_teleport);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin sidekick hosts_show_teleport`
Expected: FAIL to compile — no `hosts` field.

- [ ] **Step 3: Add HostsConfig**

In `src/config.rs`:

1. Add the field to `Config` (`src/config.rs:5-15`):

```rust
    pub hosts: HostsConfig,
```

2. Add the struct (after `EditorConfig`, `src/config.rs:58-64`):

```rust
#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub struct HostsConfig {
    /// Show Teleport nodes (from `tsh ls`) in the Hosts panel. Off by
    /// default so tsh is never invoked unless asked for.
    pub show_teleport: bool,
}
```

3. Extend `DEFAULT_CONFIG` (`src/config.rs:123-171`) — after the `[editor]` block, before the `# Global run-panel tasks` comment:

```toml
[hosts]
# Show Teleport nodes (`tsh ls`) in the Hosts panel. When false, tsh is
# never invoked at all.
show_teleport = false
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --bin sidekick hosts_show_teleport`
Expected: PASS.

- [ ] **Step 5: Write the failing hostspanel tests**

Add to the `tests` module in `src/hostspanel.rs`:

```rust
    #[test]
    fn parses_plain_and_beam_nodes() {
        let json = serde_json::json!([
            {"spec": {"hostname": "web-1"}, "metadata": {"labels": {"env": "prod"}}},
            {"spec": {"hostname": "beam-3f2a"}, "metadata": {"labels": {"teleport.internal/beams/alias": "my-beam"}}},
        ]);
        let nodes = parse_teleport_nodes(&json);
        assert_eq!(nodes.len(), 2);
        // Sorted by name: my-beam, web-1.
        assert_eq!(nodes[0].name, "my-beam");
        assert_eq!(nodes[0].command, "tsh beams ssh my-beam");
        assert_eq!(nodes[1].name, "web-1");
        assert_eq!(nodes[1].command, "tsh ssh web-1");
    }

    #[test]
    fn teleport_nodes_fall_back_to_metadata_name_and_dedupe() {
        let json = serde_json::json!([
            {"metadata": {"name": "named-only"}},
            {"spec": {"hostname": "dup"}},
            {"spec": {"hostname": "dup"}},
            {"spec": {}},
        ]);
        let nodes = parse_teleport_nodes(&json);
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["dup", "named-only"]);
    }
```

- [ ] **Step 6: Run tests to verify they fail**

Run: `cargo test --bin sidekick teleport`
Expected: FAIL to compile — `parse_teleport_nodes` / `TeleportNode` not found.

- [ ] **Step 7: Implement Beam parsing in hostspanel.rs**

Replace `teleport_nodes` (`src/hostspanel.rs:124-149`) with:

```rust
pub struct TeleportNode {
    pub name: String,
    pub command: String,
}

/// Teleport nodes from `tsh ls --format=json`, or a short status message.
fn teleport_nodes() -> Result<Vec<TeleportNode>, String> {
    let output = std::process::Command::new("tsh")
        .args(["ls", "--format=json"])
        .output()
        .map_err(|_| "tsh not installed".to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr.lines().next().unwrap_or("tsh ls failed");
        return Err(format!("tsh: {first}"));
    }
    let nodes: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "tsh: bad output".to_string())?;
    Ok(parse_teleport_nodes(&nodes))
}

/// (display name, connect command) for each node. Beam instances list as
/// nodes with a beam-<uuid> hostname that `tsh ssh` can't dial — they carry
/// a friendly alias in their labels and connect via `tsh beams ssh`.
fn parse_teleport_nodes(nodes: &serde_json::Value) -> Vec<TeleportNode> {
    let mut out: Vec<TeleportNode> = nodes
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|n| {
                    if let Some(alias) =
                        n["metadata"]["labels"]["teleport.internal/beams/alias"].as_str()
                    {
                        return Some(TeleportNode {
                            name: alias.to_string(),
                            command: format!("tsh beams ssh {alias}"),
                        });
                    }
                    let hostname = n["spec"]["hostname"]
                        .as_str()
                        .or_else(|| n["metadata"]["name"].as_str())?;
                    Some(TeleportNode {
                        name: hostname.to_string(),
                        command: format!("tsh ssh {hostname}"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}
```

- [ ] **Step 8: Run tests to verify they pass (refresh_list won't compile yet — fix in step 9)**

Run: `cargo test --bin sidekick teleport`
Expected: compile error in `refresh_list` (node type changed). Proceed.

- [ ] **Step 9: Gate the TELEPORT section behind show_teleport**

In `src/hostspanel.rs`:

1. Add the import at the top: `use std::cell::Cell;` and `use std::rc::Rc;`

2. Extend the struct and constructor:

```rust
pub struct HostsPanel {
    pub widget: gtk4::Box,
    pub list: gtk4::ListBox,
    pub refresh_btn: gtk4::Button,
    show_teleport: Rc<Cell<bool>>,
}

pub fn build(show_teleport: bool) -> HostsPanel {
    // ... existing widget construction unchanged ...

    let panel = HostsPanel {
        widget,
        list,
        refresh_btn,
        show_teleport: Rc::new(Cell::new(show_teleport)),
    };
    panel.refresh();
    {
        let list = panel.list.clone();
        let show = Rc::clone(&panel.show_teleport);
        panel.refresh_btn.connect_clicked(move |_| {
            refresh_list(&list, show.get());
        });
    }
    panel
}

impl HostsPanel {
    pub fn refresh(&self) {
        refresh_list(&self.list, self.show_teleport.get());
    }

    /// Enable or disable the Teleport section. When disabled, tsh is never
    /// invoked at all. Refreshes the list when the value changes.
    pub fn set_show_teleport(&self, show: bool) {
        if self.show_teleport.replace(show) != show {
            self.refresh();
        }
    }
}
```

3. `refresh_list` gains the flag and gates the section; the node loop uses the new type:

```rust
fn refresh_list(list: &gtk4::ListBox, show_teleport: bool) {
    let (tx, rx) = async_channel::bounded::<Vec<Item>>(1);
    std::thread::spawn(move || {
        let mut items = Vec::new();

        // ... existing SSH section unchanged ...

        if show_teleport {
            items.push(Item::Header("TELEPORT".to_string()));
            match teleport_nodes() {
                Ok(nodes) if nodes.is_empty() => {
                    items.push(Item::Message("No teleport nodes".to_string()));
                }
                Ok(nodes) => {
                    for node in nodes {
                        if !is_safe_host(&node.name) {
                            continue;
                        }
                        items.push(Item::Host {
                            name: node.name,
                            command: node.command,
                        });
                    }
                }
                Err(message) => items.push(Item::Message(message)),
            }
        }

        let _ = tx.send_blocking(items);
    });
    // ... existing receive/populate unchanged ...
}
```

- [ ] **Step 10: Wire config through main.rs**

1. Build site (`src/main.rs:213`):

```rust
    let hosts_panel = Rc::new(hostspanel::build(cfg.borrow().hosts.show_teleport));
```

(`hosts_panel.widget` at line 229 and `hosts_panel.list` at line 1070 keep working through Rc deref.)

2. `reload_config` (`src/main.rs:833-862`): add `let hosts_panel_r = Rc::clone(&hosts_panel);` to the captures, and inside the `{ let cfg_ref = cfg.borrow(); ... }` block add:

```rust
                hosts_panel_r.set_show_teleport(cfg_ref.hosts.show_teleport);
```

- [ ] **Step 11: Build, test, clippy**

Run: `cargo test --bin sidekick && cargo clippy --all-targets -- -D warnings`
Expected: PASS / clean (4 new tests in this task).

- [ ] **Step 12: Manual verification**

Run the app: Hosts panel shows only the SSH section. Add `[hosts]\nshow_teleport = true` to `~/.config/sidekick/config.toml` (or the test config) and save — the TELEPORT section appears (or shows a tsh status message) without restart. Set back to false — section disappears.

- [ ] **Step 13: Commit**

```bash
git add src/config.rs src/hostspanel.rs src/main.rs
git commit -m "feat: gate Teleport hosts behind config opt-in; support Beam aliases"
```

---

### Task 6: Agent-hook installer — PreToolUse/SessionEnd, sidekick-hook, Pi

The installer script gains: `PreToolUse → busy` (approved tool flips ready back to busy) and `SessionEnd → idle` (clears the tab from the agents panel) Claude hooks; automatic install of the `sidekick-hook` edit-review binary with a `Write|Edit|MultiEdit` matcher; and a Pi extension that reports the same OSC 666 termprop. The `idle` status is already understood end-to-end (`src/bin/sidekick-agent-status.rs`, `agent_state_from_status` at `src/main.rs:3264`).

**Files:**
- Create: `scripts/pi-sidekick-status.ts`
- Modify: `scripts/install-agent-status-hooks`
- Test: sandbox-`$HOME` run (script has no cargo test harness)

- [ ] **Step 1: Create the Pi extension**

Create `scripts/pi-sidekick-status.ts` (mirror of the macOS one — it writes to `/dev/tty`, which is platform-independent):

```typescript
// Sidekick agent-status extension for the Pi coding agent.
//
// Reports Pi's lifecycle to Sidekick's agents panel using the same OSC 666
// termprop sequence that sidekick-agent-status emits for Claude Code/Codex
// hooks. Installed by scripts/install-agent-status-hooks.
//
// Mapping:
//   agent_start            -> busy  (working on a prompt)
//   agent_end              -> done  (back at the input prompt)
//   session_shutdown(quit) -> idle  (removed from the agents panel)
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { closeSync, openSync, writeSync } from "node:fs";

const TERMPROP = "vte.ext.sidekick.agent";

function report(status: "busy" | "ready" | "done" | "idle"): void {
  try {
    const fd = openSync("/dev/tty", "w");
    try {
      writeSync(fd, `\x1b]666;${TERMPROP}=${status}\x1b\\`);
    } finally {
      closeSync(fd);
    }
  } catch {
    // No controlling terminal (print/RPC mode) — nothing to report to.
  }
}

export default function (pi: ExtensionAPI) {
  pi.on("agent_start", async () => report("busy"));
  pi.on("agent_end", async () => report("done"));
  pi.on("session_shutdown", async (event) => {
    if (event.reason === "quit") {
      report("idle");
    }
  });
}
```

- [ ] **Step 2: Build and install sidekick-hook alongside sidekick-agent-status**

In `scripts/install-agent-status-hooks`, the build/install block at the top becomes:

```bash
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin_dir="${HOME}/.local/bin"
status_bin="${bin_dir}/sidekick-agent-status"
hook_bin="${bin_dir}/sidekick-hook"

mkdir -p "$bin_dir"
cargo build --release --manifest-path "$repo/Cargo.toml" \
    --bin sidekick-agent-status --bin sidekick-hook
install -m 0755 "$repo/target/release/sidekick-agent-status" "$status_bin"
install -m 0755 "$repo/target/release/sidekick-hook" "$hook_bin"
```

- [ ] **Step 3: Extend the Claude hooks Python block**

Pass `hook_bin` as a third argument: the heredoc invocation becomes

```bash
python3 - "$HOME/.claude/settings.json" "$status_bin" "$hook_bin" <<'PY'
```

and inside the Python block: add `hook_bin = sys.argv[3]` next to `status_bin`, then replace `add_hook` and the call list with:

```python
def add_hook(event, command, matcher=None):
    groups = hooks.setdefault(event, [])
    # Dedupe by binary name + argument so an existing install at another
    # path (or by another installer) counts as already registered.
    signature = command.split("/")[-1]
    for group in groups:
        for hook in group.get("hooks", []):
            existing = hook.get("command", "")
            if existing == command or existing.endswith(signature):
                return
    group = {"hooks": [{"type": "command", "command": command}]}
    if matcher is not None:
        group["matcher"] = matcher
    groups.append(group)

add_hook("UserPromptSubmit", f"{status_bin} busy")
add_hook("PermissionRequest", f"{status_bin} ready")
# Flips "ready" back to "busy" as soon as an approved tool starts running.
add_hook("PreToolUse", f"{status_bin} busy")
add_hook("Stop", f"{status_bin} done")
# Clears the tab from the agents panel when the session ends.
add_hook("SessionEnd", f"{status_bin} idle")
# Show file edits in Sidekick for accept/reject before they're applied.
add_hook("PreToolUse", hook_bin, matcher="Write|Edit|MultiEdit")
```

(The Codex block stays as-is — Codex hooks don't expose PreToolUse/SessionEnd.)

- [ ] **Step 4: Add the Pi install step**

After the Codex Python block, before the final `cat <<EOF`:

```bash
# Pi coding agent: status extension (only when Pi is present).
pi_dir="${HOME}/.pi/agent"
if [ -d "$pi_dir" ]; then
    mkdir -p "$pi_dir/extensions"
    install -m 0644 "$repo/scripts/pi-sidekick-status.ts" \
        "$pi_dir/extensions/sidekick-status.ts"
    pi_note="  $pi_dir/extensions/sidekick-status.ts"
else
    pi_note="  (Pi not detected at ~/.pi/agent — extension skipped)"
fi
```

and extend the closing message:

```bash
cat <<EOF
Installed sidekick-agent-status and sidekick-hook to:
  $bin_dir

Updated:
  $HOME/.claude/settings.json
  $HOME/.codex/config.toml
$pi_note

Restart Claude Code/Codex/Pi sessions for hook changes to take effect.
EOF
```

- [ ] **Step 5: Verify against a sandbox HOME**

```bash
tmp=$(mktemp -d)
mkdir -p "$tmp/.pi/agent"
HOME="$tmp" scripts/install-agent-status-hooks
python3 - "$tmp" <<'PY'
import json, sys
home = sys.argv[1]
s = json.load(open(f"{home}/.claude/settings.json"))
hooks = s["hooks"]
flat = {e: [h["command"] for g in v for h in g["hooks"]] for e, v in hooks.items()}
assert any("sidekick-agent-status busy" in c for c in flat["UserPromptSubmit"])
assert any("sidekick-agent-status busy" in c for c in flat["PreToolUse"])
assert any("sidekick-agent-status idle" in c for c in flat["SessionEnd"])
assert any(c.endswith("sidekick-hook") for c in flat["PreToolUse"])
matchers = [g.get("matcher") for g in hooks["PreToolUse"]]
assert "Write|Edit|MultiEdit" in matchers
print("claude hooks OK")
PY
test -f "$tmp/.pi/agent/extensions/sidekick-status.ts" && echo "pi extension OK"
test -x "$tmp/.local/bin/sidekick-hook" && echo "sidekick-hook OK"
# Idempotency: run again, confirm no duplicate hook groups appear.
HOME="$tmp" scripts/install-agent-status-hooks > /dev/null
python3 -c "
import json
s = json.load(open('$tmp/.claude/settings.json'))
assert len(s['hooks']['PreToolUse']) == 2, s['hooks']['PreToolUse']
print('idempotent OK')
"
rm -rf "$tmp"
```

Expected output includes: `claude hooks OK`, `pi extension OK`, `sidekick-hook OK`, `idempotent OK`.

- [ ] **Step 6: Commit**

```bash
git add scripts/install-agent-status-hooks scripts/pi-sidekick-status.ts
git commit -m "feat: installer wires PreToolUse/SessionEnd hooks, sidekick-hook, and Pi extension"
```

---

### Task 7: Documentation + final verification

**Files:**
- Modify: `README.md` (shortcuts table, git panel, hosts panel, agent integration sections — exact insertion points depend on current README structure; find each section by heading)

- [ ] **Step 1: Update README.md**

Locate and update these sections (search by the listed text):

1. Keyboard shortcuts (find `Ctrl+1`): add a row
   `Ctrl+Shift+J — Jump to the next tab whose agent wants attention (waiting first, then done, then running)`.
2. Git panel section (find `push`): mention the pull button shows the behind count (`↓ pull N`), and that conflicted files appear in a CONFLICTS section — clicking one opens a marker-highlighted view, right-click → Mark resolved (stage).
3. Hosts panel section (find `Teleport` or `tsh`): document `[hosts] show_teleport = false` (opt-in; tsh never runs when off) and Beam alias support (`tsh beams ssh <alias>`).
4. Agent integration section (find `install-agent-status-hooks`): note the script now also registers `PreToolUse` (busy) and `SessionEnd` (idle) hooks, installs `sidekick-hook` with the `Write|Edit|MultiEdit` matcher automatically (replace the manual-copy instructions around `README.md:349`), and installs the Pi status extension when `~/.pi/agent` exists.

- [ ] **Step 2: Full verification**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build --release`
Expected: all clean, all tests pass, release build succeeds.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document attention jump, conflicts, pull count, teleport opt-in, installer changes"
```
