# Sidekick Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the bugs, security issues, and performance problems found in the code review, and add the agreed feature improvements.

**Architecture:** Sidekick is a single-binary GTK4/VTE Rust desktop terminal workspace plus three small helper bins (`sidekick-ctl`, `sidekick-hook`, `sidekick-agent-status`). Most logic lives in `src/main.rs`; panels and subsystems are split into focused modules. Changes here are surgical edits to existing modules plus a session-format extension. Pure-logic additions get unit tests; GTK glue gets explicit manual-verification steps.

**Tech Stack:** Rust 2021, gtk4 0.11, vte4 0.10, sourceview5, webkit6, serde/serde_json, toml, libc.

**Scope decisions (locked):**
- Theming is left entirely as-is. No config change, no opacity move. (Item dropped from review.)
- `.sidekick.toml` task trust is handled with a README note only — no confirmation/visual code.
- Socket location is moved to `$XDG_RUNTIME_DIR` with a `~/.local/run` fallback, replicated across all three binaries that compute it (matches the existing clipboard-image rationale).

**Conventions:**
- After each task: `cargo fmt && cargo test && cargo check` must pass.
- Commit after each task with the message shown.
- All file line numbers are approximate anchors; match on the quoted code, not the number.

---

## File-by-file change map

- `src/main.rs` — last-shell session clear (add_tab path), single-instance activate guard, Ctrl+F fall-through, Ctrl+1..9 tab jump, notification default action + focus-tab app action, stop-on-click for run tasks, agents-panel WAIT-first sort, bracketed paste for task paste, tree_busy guard, thread tab_id into notify helpers, config reload keep-previous, session snapshot/restore of tab names + split ratios.
- `src/gitpanel.rs` — confirm before "Discard changes".
- `src/git.rs` — `changed_files(root)` / `ahead_count(root)` signatures; `push`/`pull` no-prompt + null stdin.
- `src/quickopen.rs` — `fd --fixed-strings` (and `find` already literal-ish; keep).
- `src/browser.rs` — UTF-8-correct `urlencoded`.
- `src/config.rs` — `parse_config` + `load_checked` returning `Result`.
- `src/diff.rs` — linear `char_offsets` instead of O(n²) byte→char.
- `src/hostspanel.rs` — `is_safe_host` validation before building connect commands.
- `src/filetree.rs` — total-entry budget cap across the whole scan.
- `src/session.rs` — `TabLayout { name, root }`; `Node::Split` gains `ratio`.
- `src/bin/sidekick-hook.rs` — allow (skip preview) instead of reject when an edit is too large.
- `src/bin/sidekick-ctl.rs`, `src/ipc.rs`, `src/bin/sidekick-hook.rs` — shared XDG socket path logic.
- `README.md` — document `llm` task field, Ctrl+1..9, project-task trust note.
- repo root — untrack `nvim.log`.

---

## Task 1: Untrack nvim.log

**Files:**
- Modify: repo (git index only)

- [ ] **Step 1: Untrack the file**

```bash
git rm --cached nvim.log
```

- [ ] **Step 2: Verify it is ignored and gone from the index**

Run: `git status --porcelain nvim.log`
Expected: shows `D  nvim.log` staged; `git check-ignore nvim.log` prints `nvim.log`.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "chore: stop tracking nvim.log (already gitignored)"
```

---

## Task 2: Clear session on last-shell exit in the main-tab path

**Problem:** `split_terminal`'s `child_exited` saves an empty session before `exit(0)` (main.rs ~2817), but `add_tab_with_command`'s handler (main.rs ~2190) does not. Exiting the only un-split tab leaves a stale session that restores on next launch.

**Files:**
- Modify: `src/main.rs` (the `terminal.connect_child_exited` closure inside `add_tab_with_command`)

- [ ] **Step 1: Make the two handlers consistent**

Find in `add_tab_with_command`:

```rust
    terminal.connect_child_exited(move |_, _| {
        agent_map_close.borrow_mut().remove(&agent_key);
        if let Some(t) = weak.upgrade() {
            if pane::close_terminal(&t, &nb) {
                std::process::exit(0);
            }
        }
    });
```

Replace with:

```rust
    terminal.connect_child_exited(move |_, _| {
        agent_map_close.borrow_mut().remove(&agent_key);
        if let Some(t) = weak.upgrade() {
            if pane::close_terminal(&t, &nb) {
                // All shells exited deliberately — start fresh next launch.
                session::save(&session::Session::default());
                std::process::exit(0);
            }
        }
    });
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Manual verification**

Launch `cargo run --bin sidekick`, type `exit` in the sole tab. Relaunch.
Expected: a single fresh tab at `$HOME`, not the previous layout. Confirm `~/.local/state/sidekick/session.json` contains `{"tabs": []}` after the exit.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "fix: clear saved session when the last shell exits from a non-split tab"
```

---

## Task 3: Single-instance activate guard

**Problem:** Default `GApplication` is unique; a second `sidekick` launch re-fires `activate` in the first process, running `build_ui` again — second window, socket rebind, duplicate timers.

**Files:**
- Modify: `src/main.rs` `main()`

- [ ] **Step 1: Guard activation**

Find:

```rust
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| build_ui(app, initial_dir.as_deref()));
    app.run_with_args(&gtk_args)
```

Replace with:

```rust
    let app = Application::builder().application_id(APP_ID).build();
    let built = std::rc::Rc::new(std::cell::Cell::new(false));
    app.connect_activate(move |app| {
        if built.get() {
            // Already running: a second launch just refocuses the window.
            // (--dir on a second launch is not forwarded to the primary.)
            if let Some(win) = app.active_window() {
                win.present();
            }
            return;
        }
        built.set(true);
        build_ui(app, initial_dir.as_deref());
    });
    app.run_with_args(&gtk_args)
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Manual verification**

Run `cargo run --bin sidekick`. In another shell run the built binary again (`./target/debug/sidekick`).
Expected: no second window appears; the existing window is raised/focused. Only one `sidekick.sock`; `sidekick-ctl ping` still returns `{"ok":true}`.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "fix: make second launch refocus the existing window instead of rebuilding UI"
```

---

## Task 4: Confirm before discarding git changes

**Problem:** `gitpanel.rs` "Discard changes" runs `git restore`/`git clean -f` with no confirmation, unlike "Move to Trash" which confirms.

**Files:**
- Modify: `src/gitpanel.rs` (the `Discard changes` menu item in `show_context_menu`)

- [ ] **Step 1: Add a confirmation dialog**

Find:

```rust
        add_menu_item(&vbox, "Discard changes", &popover, {
            let path = rel_path.to_string();
            let root = root.to_string();
            let refresh = Rc::clone(on_refresh);
            let parent_w = parent.clone();
            move || match git::discard(&root, &path, is_untracked) {
                Ok(()) => refresh(),
                Err(e) => show_git_error(&parent_w, &e),
            }
        });
```

Replace with:

```rust
        add_menu_item(&vbox, "Discard changes", &popover, {
            let path = rel_path.to_string();
            let root = root.to_string();
            let refresh = Rc::clone(on_refresh);
            let parent_w = parent.clone();
            move || {
                let window = parent_w
                    .root()
                    .and_then(|r| r.downcast::<gtk4::Window>().ok());
                let detail = if is_untracked {
                    format!("{path}\n\nThis untracked file will be deleted. This cannot be undone.")
                } else {
                    format!("{path}\n\nLocal changes to this file will be lost. This cannot be undone.")
                };
                let path = path.clone();
                let root = root.clone();
                let refresh = Rc::clone(&refresh);
                let parent_w = parent_w.clone();
                gtk4::AlertDialog::builder()
                    .message("Discard changes?")
                    .detail(detail)
                    .buttons(["Cancel", "Discard"])
                    .cancel_button(0)
                    .default_button(0)
                    .build()
                    .choose(window.as_ref(), None::<&gio::Cancellable>, move |choice| {
                        if choice == Ok(1) {
                            match git::discard(&root, &path, is_untracked) {
                                Ok(()) => refresh(),
                                Err(e) => show_git_error(&parent_w, &e),
                            }
                        }
                    });
            }
        });
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles (note `gio` is already a dependency; `gio::Cancellable` is referenced fully-qualified).

- [ ] **Step 3: Manual verification**

Modify a tracked file, right-click it in the git panel → Discard changes. Confirm a dialog appears; Cancel leaves the file changed; Discard reverts it.

- [ ] **Step 4: Commit**

```bash
git add src/gitpanel.rs
git commit -m "fix: confirm before discarding git changes"
```

---

## Task 5: Ctrl+F falls through to the shell when quick-open won't open

**Problem:** main.rs ~1151 returns `Stop` even when quick-open declines (home dir / empty cwd), swallowing the key.

**Files:**
- Modify: `src/main.rs` (the `(true, false, false, gdk::Key::f | gdk::Key::F)` arm)

- [ ] **Step 1: Return Proceed when quick-open does not open**

Find:

```rust
                // Quick open: file name search
                (true, false, false, gdk::Key::f | gdk::Key::F) => {
                    let cwd = last_cwd_qo.borrow().clone();
                    if !cwd.is_empty() {
                        let repo = git::repo_root(&cwd);
                        let home = std::env::var("HOME").unwrap_or_default();
                        // Outside a repo, refuse to index the entire home dir.
                        if repo.is_some() || cwd != home {
                            let root = repo.unwrap_or(cwd);
                            if let Some(on_saved) = on_saved_k.borrow().as_ref() {
                                quickopen::show(&root, &win, &nb, &cfg_qo, Rc::clone(on_saved));
                            }
                        }
                    }
                    glib::Propagation::Stop
                }
```

Replace with:

```rust
                // Quick open: file name search. If we cannot open (no cwd, or
                // bare home dir with no repo), let Ctrl+F reach the shell.
                (true, false, false, gdk::Key::f | gdk::Key::F) => {
                    let cwd = last_cwd_qo.borrow().clone();
                    if cwd.is_empty() {
                        return glib::Propagation::Proceed;
                    }
                    let repo = git::repo_root(&cwd);
                    let home = std::env::var("HOME").unwrap_or_default();
                    if repo.is_none() && cwd == home {
                        return glib::Propagation::Proceed;
                    }
                    let root = repo.unwrap_or(cwd);
                    if let Some(on_saved) = on_saved_k.borrow().as_ref() {
                        quickopen::show(&root, &win, &nb, &cfg_qo, Rc::clone(on_saved));
                    }
                    glib::Propagation::Stop
                }
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Manual verification**

In a shell at `$HOME` (not a repo), press Ctrl+F at a prompt with text — cursor moves forward one char (readline) instead of nothing. `cd` into a repo, Ctrl+F opens quick-open.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "fix: let Ctrl+F reach the shell when quick-open declines to open"
```

---

## Task 6: git push/pull must not hang on credential prompts

**Files:**
- Modify: `src/git.rs` `push` and `pull`

- [ ] **Step 1: Add no-prompt env and null stdin**

In `pull`, find:

```rust
    let out = Command::new("git")
        .args(["-C", &root, "pull"])
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
```

Replace with:

```rust
    let out = Command::new("git")
        .args(["-C", &root, "pull"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
```

In `push`, find:

```rust
    let out = Command::new("git")
        .args(["-C", &root, "push"])
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
```

Replace with:

```rust
    let out = Command::new("git")
        .args(["-C", &root, "push"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Manual verification**

In a repo whose remote needs credentials you don't have cached, click push. It returns a failure dialog promptly instead of the button sticking on "pushing…" forever.

- [ ] **Step 4: Commit**

```bash
git add src/git.rs
git commit -m "fix: prevent git push/pull from hanging on credential prompts"
```

---

## Task 7: changed_files / ahead_count take a known root (fewer git spawns)

**Files:**
- Modify: `src/git.rs`, `src/main.rs` (two call sites)

- [ ] **Step 1: Change `changed_files` to take `root`**

Find:

```rust
pub fn changed_files(cwd: &str) -> Vec<GitFile> {
    let root = match repo_root(cwd) {
        Some(r) => r,
        None => return vec![],
    };
    // -z gives NUL-delimited, unquoted paths (porcelain v1 C-quotes paths with
```

Replace the signature and drop the internal `repo_root`:

```rust
pub fn changed_files(root: &str) -> Vec<GitFile> {
    // -z gives NUL-delimited, unquoted paths (porcelain v1 C-quotes paths with
```

Then change the `git status` invocation from `["-C", &root, "status", ...]` to `["-C", root, "status", ...]`. The `format!("{}/{}", root, rel)` lines remain valid.

- [ ] **Step 2: Change `ahead_count` to take `root`**

Find:

```rust
pub fn ahead_count(cwd: &str) -> u32 {
    let root = match repo_root(cwd) {
        Some(r) => r,
        None => return 0,
    };
    let out = Command::new("git")
        .args(["-C", &root, "rev-list", "--count", "@{u}..HEAD"])
```

Replace with:

```rust
pub fn ahead_count(root: &str) -> u32 {
    let out = Command::new("git")
        .args(["-C", root, "rev-list", "--count", "@{u}..HEAD"])
```

- [ ] **Step 3: Update the two call sites in `src/main.rs`**

In `refresh_git` find:

```rust
                let root = git::repo_root(&cwd).unwrap_or_else(|| cwd.clone());
                let files = git::changed_files(&cwd);
                let ahead = git::ahead_count(&cwd);
                let branch = git::current_branch(&root);
```

Replace with:

```rust
                let root = git::repo_root(&cwd).unwrap_or_else(|| cwd.clone());
                let files = git::changed_files(&root);
                let ahead = git::ahead_count(&root);
                let branch = git::current_branch(&root);
```

In the 5-second git timer find the identical block and apply the same change (`&cwd` → `&root` for `changed_files` and `ahead_count`).

- [ ] **Step 4: Verify build and tests**

Run: `cargo check && cargo test`
Expected: compiles, tests pass.

- [ ] **Step 5: Manual verification**

Open the git panel in a repo with changes and a tracked upstream; the file list, change count, push-ahead count, and branch label all still populate correctly.

- [ ] **Step 6: Commit**

```bash
git add src/git.rs src/main.rs
git commit -m "perf: avoid redundant git repo_root spawns by passing known root"
```

---

## Task 8: Quick-open matches literally (fd --fixed-strings)

**Files:**
- Modify: `src/quickopen.rs` `search_files`

- [ ] **Step 1: Add `--fixed-strings` to the fd invocation**

Find:

```rust
        std::process::Command::new("fd")
            .args([
                "--type",
                "f",
                "--max-results",
                &max,
                "--color",
                "never",
                "--",
                query,
            ])
```

Replace with:

```rust
        std::process::Command::new("fd")
            .args([
                "--type",
                "f",
                "--fixed-strings",
                "--max-results",
                &max,
                "--color",
                "never",
                "--",
                query,
            ])
```

(The `find` fallback uses `-iname "*query*"` glob matching, which treats the query literally apart from shell-glob metacharacters — acceptable, left as-is.)

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Manual verification**

With `fd` installed, open quick-open (Ctrl+F) in a repo and type a filename fragment containing `(` or `.` — results appear instead of an empty list / regex error.

- [ ] **Step 4: Commit**

```bash
git add src/quickopen.rs
git commit -m "fix: search quick-open file names literally (fd --fixed-strings)"
```

---

## Task 9: UTF-8-correct browser URL encoding

**Files:**
- Modify: `src/browser.rs` `urlencoded` + add a test

- [ ] **Step 1: Write the failing test**

Add at the bottom of `src/browser.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::urlencoded;

    #[test]
    fn encodes_utf8_bytes_not_codepoints() {
        assert_eq!(urlencoded("a b"), "a+b");
        assert_eq!(urlencoded("rust lang"), "rust+lang");
        // € is U+20AC = E2 82 AC in UTF-8
        assert_eq!(urlencoded("€"), "%E2%82%AC");
        assert_eq!(urlencoded("naïve"), "na%C3%AFve");
        assert_eq!(urlencoded("a-_.~z"), "a-_.~z");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --bin sidekick urlencoded`
Expected: FAIL (current impl encodes codepoints).

- [ ] **Step 3: Rewrite `urlencoded` to encode UTF-8 bytes**

Find:

```rust
fn urlencoded(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => '+'.to_string(),
            c if c.is_alphanumeric() || "-_.~".contains(c) => c.to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}
```

Replace with:

```rust
fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b' ' => out.push('+'),
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --bin sidekick urlencoded`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/browser.rs
git commit -m "fix: percent-encode UTF-8 bytes in browser search queries"
```

---

## Task 10: Config reload keeps the previous config on parse error

**Files:**
- Modify: `src/config.rs` (add `parse_config` + `load_checked`, refactor `load`), `src/main.rs` (`reload_config`)

- [ ] **Step 1: Write the failing test**

Add a `tests` module at the bottom of `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_config() {
        let cfg = parse_config("[font]\nsize = 20\n").expect("valid");
        assert_eq!(cfg.font.size, 20);
    }

    #[test]
    fn invalid_config_is_err() {
        assert!(parse_config("this is = = not toml").is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bin sidekick parse_config`
Expected: FAIL (`parse_config` not defined).

- [ ] **Step 3: Add `parse_config` and `load_checked`, refactor `load`**

Find:

```rust
impl Config {
    pub fn load() -> Self {
        let path = config_path();

        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, DEFAULT_CONFIG);
            return Self::default();
        }

        let content = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };

        toml::from_str(&content).unwrap_or_default()
    }
}
```

Replace with:

```rust
impl Config {
    /// Startup load: never fails, falls back to defaults on any error.
    pub fn load() -> Self {
        Self::load_checked().unwrap_or_default()
    }

    /// Load returning an error for malformed TOML, so reloads can keep the
    /// previous in-memory config instead of silently resetting to defaults.
    pub fn load_checked() -> Result<Self, String> {
        let path = config_path();

        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, DEFAULT_CONFIG);
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        parse_config(&content)
    }
}

pub fn parse_config(content: &str) -> Result<Config, String> {
    toml::from_str(content).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Run to verify the test passes**

Run: `cargo test --bin sidekick parse_config`
Expected: PASS.

- [ ] **Step 5: Use `load_checked` in `reload_config`**

In `src/main.rs` find:

```rust
        move || {
            let next = config::Config::load();
            *cfg.borrow_mut() = next;
```

Replace with:

```rust
        move || {
            let next = match config::Config::load_checked() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("sidekick: config reload failed, keeping previous config: {e}");
                    return;
                }
            };
            *cfg.borrow_mut() = next;
```

- [ ] **Step 6: Verify build and tests**

Run: `cargo check && cargo test`
Expected: compiles, tests pass.

- [ ] **Step 7: Manual verification**

With sidekick running, introduce a TOML syntax error in `~/.config/sidekick/config.toml` and save. The UI does not reset to defaults; a warning is printed to stderr. Fix the error and save — config applies.

- [ ] **Step 8: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "fix: keep previous config when a live reload fails to parse"
```

---

## Task 11: sidekick-hook allows (skips preview for) over-large edits

**Problem:** Over-limit edits currently `exit(2)` (= reject), even when sidekick isn't running. They should proceed without a preview.

**Files:**
- Modify: `src/bin/sidekick-hook.rs` (`extract_edits`, three sites)

- [ ] **Step 1: Replace the three reject-on-too-large sites with skip-preview**

In the `"Write"` arm find:

```rust
            if new.len() > MAX_HOOK_TEXT_BYTES as usize {
                eprintln!("sidekick-hook: edit too large to preview");
                std::process::exit(2);
            }
```

Replace with:

```rust
            if new.len() > MAX_HOOK_TEXT_BYTES as usize {
                eprintln!("sidekick-hook: edit too large to preview, allowing without preview");
                return vec![];
            }
```

In the `"Edit"` arm find the identical `if new_content.len() > MAX_HOOK_TEXT_BYTES ...` block and replace its body the same way (message + `return vec![];`).

In the `"MultiEdit"` arm find the identical `if current.len() > MAX_HOOK_TEXT_BYTES ...` block (inside the loop) and replace the body with the message + `return vec![];`.

- [ ] **Step 2: Verify build**

Run: `cargo check --bin sidekick-hook`
Expected: compiles.

- [ ] **Step 3: Manual verification**

```bash
python3 -c 'import json,sys; sys.stdout.write(json.dumps({"tool_name":"Write","tool_input":{"file_path":"/tmp/x","content":"a"*(5*1024*1024)}}))' | ./target/debug/sidekick-hook; echo "exit=$?"
```
Expected: `exit=0`.

- [ ] **Step 4: Commit**

```bash
git add src/bin/sidekick-hook.rs
git commit -m "fix: allow over-large edits without preview instead of rejecting them"
```

---

## Task 12: Linear diff tagging (remove O(n²) byte→char)

**Files:**
- Modify: `src/diff.rs` (add `char_offsets`, use it in `open` and `open_readonly`, add test)

- [ ] **Step 1: Write the failing test**

Add at the bottom of `src/diff.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::char_offsets;

    fn naive(text: &str, b: usize) -> usize {
        text[..b.min(text.len())].chars().count()
    }

    #[test]
    fn char_offsets_matches_naive_with_multibyte() {
        let text = "héllo\nwörld\n€nd\n";
        let bytes = vec![0, 3, 6, 9, text.len(), text.len() + 5];
        let got = char_offsets(text, &bytes);
        let want: Vec<usize> = bytes.iter().map(|&b| naive(text, b)).collect();
        assert_eq!(got, want);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bin sidekick char_offsets`
Expected: FAIL (`char_offsets` not defined).

- [ ] **Step 3: Add `char_offsets` and replace the naive helper**

Find:

```rust
fn byte_offset_to_char_offset(s: &str, byte_offset: usize) -> usize {
    s[..byte_offset.min(s.len())].chars().count()
}
```

Replace with:

```rust
/// Convert a list of byte offsets into char offsets in a single pass.
/// `byte_offsets` must be non-decreasing (diff spans are emitted in order).
fn char_offsets(text: &str, byte_offsets: &[usize]) -> Vec<usize> {
    let mut result = Vec::with_capacity(byte_offsets.len());
    let mut cur_byte = 0usize;
    let mut cur_char = 0usize;
    for &b in byte_offsets {
        let b = b.min(text.len());
        if b >= cur_byte {
            cur_char += text[cur_byte..b].chars().count();
        } else {
            // Out-of-order fallback (not expected): recompute from start.
            cur_char = text[..b].chars().count();
        }
        cur_byte = b;
        result.push(cur_char);
    }
    result
}
```

- [ ] **Step 4: Use it in `open`**

Find:

```rust
    buffer.set_text(&text);
    for (start_byte, end_byte, tag_name) in spans {
        let start_iter =
            buffer.iter_at_offset(byte_offset_to_char_offset(&text, start_byte) as i32);
        let end_iter = buffer.iter_at_offset(byte_offset_to_char_offset(&text, end_byte) as i32);
        buffer.apply_tag_by_name(tag_name, &start_iter, &end_iter);
    }
```

Replace with:

```rust
    buffer.set_text(&text);
    let mut boundaries = Vec::with_capacity(spans.len() * 2);
    for (s, e, _) in &spans {
        boundaries.push(*s);
        boundaries.push(*e);
    }
    let chars = char_offsets(&text, &boundaries);
    for (i, (_, _, tag_name)) in spans.iter().enumerate() {
        let start_iter = buffer.iter_at_offset(chars[i * 2] as i32);
        let end_iter = buffer.iter_at_offset(chars[i * 2 + 1] as i32);
        buffer.apply_tag_by_name(tag_name, &start_iter, &end_iter);
    }
```

- [ ] **Step 5: Use it in `open_readonly`**

Find:

```rust
    buffer.set_text(&text);
    for (sb, eb, tag) in spans {
        let si = buffer.iter_at_offset(byte_offset_to_char_offset(&text, sb) as i32);
        let ei = buffer.iter_at_offset(byte_offset_to_char_offset(&text, eb) as i32);
        buffer.apply_tag_by_name(tag, &si, &ei);
    }
```

Replace with:

```rust
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
```

- [ ] **Step 6: Run tests**

Run: `cargo test --bin sidekick`
Expected: PASS including `char_offsets_matches_naive_with_multibyte`.

- [ ] **Step 7: Manual verification**

Open a large changed file as a diff (git panel) and an accept/reject diff via the hook; colors line up exactly and the tab opens promptly.

- [ ] **Step 8: Commit**

```bash
git add src/diff.rs
git commit -m "perf: tag diff views in linear time instead of O(n^2) byte->char"
```

---

## Task 13: Validate ssh/teleport hostnames before building shell commands

**Files:**
- Modify: `src/hostspanel.rs` (add `is_safe_host`, filter both sources, add test)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module of `src/hostspanel.rs`:

```rust
    #[test]
    fn rejects_unsafe_host_names() {
        assert!(is_safe_host("dev"));
        assert!(is_safe_host("db.example.com"));
        assert!(is_safe_host("user@host-1"));
        assert!(!is_safe_host("x; curl evil | sh"));
        assert!(!is_safe_host("a b"));
        assert!(!is_safe_host("$(whoami)"));
        assert!(!is_safe_host(""));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bin sidekick is_safe_host`
Expected: FAIL (`is_safe_host` not defined).

- [ ] **Step 3: Add `is_safe_host`**

Add near `parse_ssh_config`:

```rust
/// Hostnames we are willing to interpolate into a shell command. Conservative
/// allowlist: letters, digits, and the punctuation that appears in real host
/// aliases / Teleport node names. Anything else is dropped to avoid command
/// injection when the row is activated (the command is fed to a shell).
pub fn is_safe_host(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@'))
}
```

- [ ] **Step 4: Filter ssh hosts**

In `refresh_list`, find:

```rust
        for host in ssh_hosts {
            items.push(Item::Host {
                command: format!("ssh {host}"),
                name: host,
            });
        }
```

Replace with:

```rust
        for host in ssh_hosts {
            if !is_safe_host(&host) {
                continue;
            }
            items.push(Item::Host {
                command: format!("ssh {host}"),
                name: host,
            });
        }
```

- [ ] **Step 5: Filter teleport nodes**

Find:

```rust
            Ok(nodes) => {
                for node in nodes {
                    items.push(Item::Host {
                        command: format!("tsh ssh {node}"),
                        name: node,
                    });
                }
            }
```

Replace with:

```rust
            Ok(nodes) => {
                for node in nodes {
                    if !is_safe_host(&node) {
                        continue;
                    }
                    items.push(Item::Host {
                        command: format!("tsh ssh {node}"),
                        name: node,
                    });
                }
            }
```

- [ ] **Step 6: Run tests**

Run: `cargo test --bin sidekick`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/hostspanel.rs
git commit -m "security: drop hosts with unsafe names before building connect commands"
```

---

## Task 14: Total-entry budget for the file-tree scan

**Files:**
- Modify: `src/filetree.rs` (add `MAX_TREE_TOTAL_ENTRIES`, thread a budget through `scan_dir`, add test)

- [ ] **Step 1: Write the failing test**

Add at the bottom of `src/filetree.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::scan_dir_budgeted;
    use std::collections::HashSet;

    #[test]
    fn scan_respects_total_budget() {
        let dir = std::env::temp_dir().join(format!("sidekick-tree-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..10 {
            std::fs::write(dir.join(format!("f{i}.txt")), b"x").unwrap();
        }
        let ignored: HashSet<String> = HashSet::new();
        let mut budget = 3usize;
        let entries = scan_dir_budgeted(dir.to_str().unwrap(), 0, &ignored, &mut budget);
        std::fs::remove_dir_all(&dir).ok();
        assert!(entries.len() <= 3, "got {} entries", entries.len());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bin sidekick scan_respects_total_budget`
Expected: FAIL (`scan_dir_budgeted` not defined).

- [ ] **Step 3: Add the budget const and rework `scan_dir`**

Add near the other consts:

```rust
const MAX_TREE_TOTAL_ENTRIES: usize = 5000;
```

Replace `scan_root`:

```rust
pub fn scan_root(root: &str) -> Vec<TreeEntry> {
    let ignored = crate::git::ignored_set(root);
    scan_dir(root, 0, &ignored)
}
```

with:

```rust
pub fn scan_root(root: &str) -> Vec<TreeEntry> {
    let ignored = crate::git::ignored_set(root);
    let mut budget = MAX_TREE_TOTAL_ENTRIES;
    scan_dir_budgeted(root, 0, &ignored, &mut budget)
}
```

Replace `scan_subtree`:

```rust
pub fn scan_subtree(path: &str) -> Vec<TreeEntry> {
    let root = crate::git::repo_root(path).unwrap_or_else(|| path.to_string());
    let ignored = crate::git::ignored_set(&root);
    scan_dir(path, 0, &ignored)
}
```

with:

```rust
pub fn scan_subtree(path: &str) -> Vec<TreeEntry> {
    let root = crate::git::repo_root(path).unwrap_or_else(|| path.to_string());
    let ignored = crate::git::ignored_set(&root);
    let mut budget = MAX_TREE_TOTAL_ENTRIES;
    scan_dir_budgeted(path, 0, &ignored, &mut budget)
}
```

Rename `scan_dir` and add the budget. Find:

```rust
fn scan_dir(path: &str, depth: u32, ignored: &std::collections::HashSet<String>) -> Vec<TreeEntry> {
    let mut entries: Vec<TreeEntry> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .take(crate::limits::MAX_DIRECTORY_ENTRIES)
        .filter_map(|e| {
```

Replace with:

```rust
fn scan_dir_budgeted(
    path: &str,
    depth: u32,
    ignored: &std::collections::HashSet<String>,
    budget: &mut usize,
) -> Vec<TreeEntry> {
    if *budget == 0 {
        return Vec::new();
    }
    let take = (*budget).min(crate::limits::MAX_DIRECTORY_ENTRIES);
    let mut entries: Vec<TreeEntry> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .take(take)
        .filter_map(|e| {
```

Then find:

```rust
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));

    for entry in &mut entries {
        if !entry.is_dir || entry.ignored {
            continue;
        }
        if depth < MAX_DEPTH {
            entry.children = scan_dir(&entry.path, depth + 1, ignored);
        } else {
```

Replace with:

```rust
    *budget = budget.saturating_sub(entries.len());
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));

    for entry in &mut entries {
        if !entry.is_dir || entry.ignored {
            continue;
        }
        if *budget == 0 {
            break;
        }
        if depth < MAX_DEPTH {
            entry.children = scan_dir_budgeted(&entry.path, depth + 1, ignored, budget);
        } else {
```

- [ ] **Step 4: Run tests**

Run: `cargo test --bin sidekick`
Expected: PASS including the new budget test.

- [ ] **Step 5: Manual verification**

`cd` into a large repo; the file tree populates without a noticeable freeze, and deep/huge trees stop expanding once the budget is exhausted (no crash).

- [ ] **Step 6: Commit**

```bash
git add src/filetree.rs
git commit -m "perf: cap total file-tree entries scanned to bound large repos"
```

---

## Task 15: Use the tree_busy guard to avoid overlapping scans

**Files:**
- Modify: `src/main.rs` (the 1-second cwd poll)

- [ ] **Step 1: Skip a scan while one is already running**

Find:

```rust
            if let Some(cwd) = focused_terminal_cwd(&win, &nb) {
                let mut prev = last.borrow_mut();
                if *prev != cwd {
                    *prev = cwd.clone();
                    tree_busy_c.set(true);
```

Replace with:

```rust
            if let Some(cwd) = focused_terminal_cwd(&win, &nb) {
                let mut prev = last.borrow_mut();
                if *prev != cwd && !tree_busy_c.get() {
                    *prev = cwd.clone();
                    tree_busy_c.set(true);
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix: guard file-tree refresh with tree_busy to avoid overlapping scans"
```

---

## Task 16: Ctrl+1..9 jumps to tab N

**Files:**
- Modify: `src/main.rs` (key handler + helper), `src/shortcutshelp.rs`, `README.md`

- [ ] **Step 1: Add a digit-key helper**

Add near the other free functions in `src/main.rs` (e.g. above `is_known_agent_command`):

```rust
/// Map Ctrl+1..Ctrl+9 to a zero-based tab index (1 -> 0, ... 9 -> 8).
fn tab_jump_index(key: gdk::Key) -> Option<u32> {
    match key {
        gdk::Key::_1 => Some(0),
        gdk::Key::_2 => Some(1),
        gdk::Key::_3 => Some(2),
        gdk::Key::_4 => Some(3),
        gdk::Key::_5 => Some(4),
        gdk::Key::_6 => Some(5),
        gdk::Key::_7 => Some(6),
        gdk::Key::_8 => Some(7),
        gdk::Key::_9 => Some(8),
        _ => None,
    }
}
```

- [ ] **Step 2: Add the key arm**

In the key handler `match (ctrl, shift, alt, key)`, add this arm immediately before the final `_ => glib::Propagation::Proceed,`:

```rust
                // Jump to tab N (Ctrl+1 .. Ctrl+9)
                (true, false, false, k) if tab_jump_index(k).is_some() => {
                    if let Some(i) = tab_jump_index(k) {
                        if i < nb.n_pages() {
                            nb.set_current_page(Some(i));
                        }
                    }
                    glib::Propagation::Stop
                }
```

- [ ] **Step 3: Document in the shortcuts help**

In `src/shortcutshelp.rs`, in the `"Tabs"` section array, add after the New-tab entry:

```rust
            ("Ctrl+1 … Ctrl+9", "Jump to tab 1–9"),
```

- [ ] **Step 4: Document in the README**

In the Keyboard Shortcuts table in `README.md`, add a row after the `Ctrl+Shift+Tab` row:

```
| `Ctrl+1` … `Ctrl+9` | Jump to tab 1–9 |
```

- [ ] **Step 5: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 6: Manual verification**

Open 3+ tabs; Ctrl+2 selects the second tab, Ctrl+9 with fewer than 9 tabs does nothing (no crash).

- [ ] **Step 7: Commit**

```bash
git add src/main.rs src/shortcutshelp.rs README.md
git commit -m "feat: Ctrl+1..9 jumps directly to tab N"
```

---

## Task 17: Clicking an agent/command notification focuses its tab

**Files:**
- Modify: `src/main.rs` (register `focus-tab` app action; set default action on both notifications; thread `tab_id` into `notify_long_command_finished` and the agent notify call)

- [ ] **Step 1: Register the `focus-tab` action in `build_ui`**

After the `window` is built (just after the `ApplicationWindow::builder()...build();` block, before the close-request handler), add:

```rust
    // App action so agent/command notifications can focus their tab on click.
    {
        use gio::prelude::ActionMapExt;
        let nb = notebook.clone();
        let win = window.clone();
        let agent_map_act = Rc::clone(&agent_map);
        let action = gio::SimpleAction::new("focus-tab", Some(glib::VariantTy::UINT64));
        action.connect_activate(move |_, param| {
            let Some(target) = param.and_then(|p| p.get::<u64>()) else {
                return;
            };
            for i in 0..nb.n_pages() {
                let Some(page) = nb.nth_page(Some(i)) else {
                    continue;
                };
                let Some(term) = pane::collect_terminals_pub(&page).into_iter().next() else {
                    continue;
                };
                let key = term.as_ptr() as usize;
                let matches = agent_map_act
                    .borrow()
                    .get(&key)
                    .map(|(id, _)| *id == target)
                    .unwrap_or(false);
                if matches {
                    nb.set_current_page(Some(i));
                    win.present();
                    term.grab_focus();
                    break;
                }
            }
        });
        app.add_action(&action);
    }
```

- [ ] **Step 2: Give `notify_agent_attention` a tab id and set the default action**

Change the signature. Find:

```rust
fn notify_agent_attention(
    notebook: &Notebook,
    key: usize,
    state: AgentState,
    title: &str,
    detail: &str,
) {
```

Replace with:

```rust
fn notify_agent_attention(
    notebook: &Notebook,
    key: usize,
    tab_id: u64,
    state: AgentState,
    title: &str,
    detail: &str,
) {
```

Inside it, find:

```rust
    let notification = gio::Notification::new(summary);
    notification.set_body(Some(&format!("{title} — {detail}")));
    // One notification id per terminal so updates replace instead of stack.
    app.send_notification(Some(&format!("sidekick-agent-{key}")), &notification);
```

Replace with:

```rust
    let notification = gio::Notification::new(summary);
    notification.set_body(Some(&format!("{title} — {detail}")));
    notification.set_default_action_and_target_value("app.focus-tab", &tab_id.to_variant());
    // One notification id per terminal so updates replace instead of stack.
    app.send_notification(Some(&format!("sidekick-agent-{key}")), &notification);
```

- [ ] **Step 3: Update the agent-notify call site**

In the per-tab 500 ms timer, find:

```rust
                if needs_attention {
                    notify_agent_attention(&nb_ref, agent_key, state, &title_text, &detail_text);
                }
```

Replace with:

```rust
                if needs_attention {
                    let tab_id = agent_map_ref
                        .borrow()
                        .get(&agent_key)
                        .map(|(id, _)| *id)
                        .unwrap_or(0);
                    notify_agent_attention(
                        &nb_ref, agent_key, tab_id, state, &title_text, &detail_text,
                    );
                }
```

- [ ] **Step 4: Thread tab id into `wire_agent_state_handlers`**

Change the signature. Find:

```rust
fn wire_agent_state_handlers(
    terminal: &vte4::Terminal,
    agent_state: &AgentCell,
    dirty_ctx: Option<(Rc<Cell<bool>>, Notebook)>,
) {
```

Replace with:

```rust
fn wire_agent_state_handlers(
    terminal: &vte4::Terminal,
    agent_state: &AgentCell,
    tab_id: u64,
    dirty_ctx: Option<(Rc<Cell<bool>>, Notebook)>,
) {
```

Inside, find:

```rust
                if !was_explicit_busy && duration.as_secs() >= LONG_COMMAND_NOTIFY_SECS {
                    notify_long_command_finished(&term_c, duration);
                }
```

Replace with:

```rust
                if !was_explicit_busy && duration.as_secs() >= LONG_COMMAND_NOTIFY_SECS {
                    notify_long_command_finished(&term_c, tab_id, duration);
                }
```

- [ ] **Step 5: Update both `wire_agent_state_handlers` call sites**

In `add_tab_with_command` find:

```rust
    wire_agent_state_handlers(
        &terminal,
        &agent_state,
        Some((Rc::clone(&dirty), notebook.clone())),
    );
```

Replace with:

```rust
    wire_agent_state_handlers(
        &terminal,
        &agent_state,
        tab_id,
        Some((Rc::clone(&dirty), notebook.clone())),
    );
```

In `split_terminal` find:

```rust
    wire_agent_state_handlers(&new_term, &agent_state, None);
```

Replace with:

```rust
    wire_agent_state_handlers(&new_term, &agent_state, tab_id, None);
```

- [ ] **Step 6: Give `notify_long_command_finished` a tab id and set the default action**

Find:

```rust
fn notify_long_command_finished(terminal: &vte4::Terminal, duration: Duration) {
```

Replace with:

```rust
fn notify_long_command_finished(terminal: &vte4::Terminal, tab_id: u64, duration: Duration) {
```

Inside it, find:

```rust
    let notification = gio::Notification::new(&summary);
    notification.set_body(Some(&format!(
        "{} — {place}",
        agentpanel::format_elapsed(duration.as_secs())
    )));
    // One notification id per terminal so updates replace instead of stack.
    let key = terminal.as_ptr() as usize;
    app.send_notification(Some(&format!("sidekick-cmd-{key}")), &notification);
```

Replace with:

```rust
    let notification = gio::Notification::new(&summary);
    notification.set_body(Some(&format!(
        "{} — {place}",
        agentpanel::format_elapsed(duration.as_secs())
    )));
    notification.set_default_action_and_target_value("app.focus-tab", &tab_id.to_variant());
    // One notification id per terminal so updates replace instead of stack.
    let key = terminal.as_ptr() as usize;
    app.send_notification(Some(&format!("sidekick-cmd-{key}")), &notification);
```

- [ ] **Step 7: Verify build**

Run: `cargo check`
Expected: compiles. (`glib::VariantTy`, `to_variant`, and `ActionMapExt` come from the `gio`/`glib` preludes already imported; if `to_variant` is unresolved, add `use glib::prelude::ToVariant;` inside `build_ui`.)

- [ ] **Step 8: Manual verification**

With an agent hook driving status, unfocus the window and let an agent reach WAIT; click the desktop notification. The sidekick window raises and the correct tab is focused. Same for a long command finishing while unfocused.

- [ ] **Step 9: Commit**

```bash
git add src/main.rs
git commit -m "feat: clicking an agent/command notification focuses its tab"
```

---

## Task 18: Stop a running run-panel task by clicking its status indicator

**Files:**
- Modify: `src/main.rs` (`track_task_status`)

- [ ] **Step 1: Make the running indicator clickable to terminate the task**

Find the whole `track_task_status` function and replace it with:

```rust
fn track_task_status(terminal: &vte4::Terminal, pid_cell: Rc<Cell<i32>>, label: &gtk4::Label) {
    label.set_markup("<span foreground=\"#f9e2af\">●</span>");
    label.set_tooltip_text(Some("running — click to stop"));

    // Click the running indicator to SIGTERM the task's foreground group.
    let stop_gesture = gtk4::GestureClick::new();
    {
        let term_for_stop = terminal.clone();
        stop_gesture.connect_pressed(move |_, _, _, _| {
            if let Some(pgid) = terminal_foreground_pgid(&term_for_stop) {
                unsafe {
                    libc::killpg(pgid, libc::SIGTERM);
                }
            }
        });
    }
    label.add_controller(stop_gesture);

    let term_weak = terminal.downgrade();
    let label_weak = label.downgrade();
    let started = Cell::new(false);
    let begun = Instant::now();
    glib::timeout_add_local(Duration::from_millis(1000), move || {
        let (Some(term), Some(label)) = (term_weak.upgrade(), label_weak.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        let pid = pid_cell.get();
        if pid < 0 {
            label.set_text("");
            return glib::ControlFlow::Break;
        }
        if pid == 0 {
            return glib::ControlFlow::Continue;
        }
        let running = terminal_has_foreground_process(&term, pid);
        if running {
            started.set(true);
            glib::ControlFlow::Continue
        } else if started.get() || begun.elapsed() > Duration::from_secs(3) {
            // Either the command ended, or it finished faster than our poll.
            label.set_markup("<span foreground=\"#a6e3a1\">✓</span>");
            label.set_tooltip_text(Some("finished"));
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Manual verification**

Define a task `cmd = "sleep 60"` in `.sidekick.toml`, click ▶ to run it in a split, then click the yellow ● status dot in the run panel. The sleep terminates and the indicator flips to ✓.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: stop a running task by clicking its run-panel status indicator"
```

---

## Task 19: Agents panel lists WAIT (waiting-for-input) tabs first

**Files:**
- Modify: `src/main.rs` (the agents-dashboard 1-second timer, after `rows` is built)

- [ ] **Step 1: Sort rows so waiting agents float to the top**

Find:

```rust
            since.borrow_mut().retain(|id, _| live_tabs.contains(id));
            if vis.get() && stack.visible_child_name().as_deref() == Some("agents") {
                panel.populate(&rows);
            }
```

Replace with:

```rust
            since.borrow_mut().retain(|id, _| live_tabs.contains(id));
            // Waiting-for-input tabs first (longest wait first), then the rest
            // in tab order (stable sort preserves tab order within a group).
            rows.sort_by(|a, b| {
                let a_wait = a.state_label == "WAIT";
                let b_wait = b.state_label == "WAIT";
                match (a_wait, b_wait) {
                    (true, true) => b.elapsed_secs.cmp(&a.elapsed_secs),
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    (false, false) => std::cmp::Ordering::Equal,
                }
            });
            if vis.get() && stack.visible_child_name().as_deref() == Some("agents") {
                panel.populate(&rows);
            }
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Manual verification**

Run several agents; the ones in WAIT appear at the top, longest-waiting first; clicking a row still jumps to the right tab (the row carries `page_index`, unaffected by sort).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: sort agents panel so waiting-for-input tabs come first"
```

---

## Task 20: Bracketed paste for multi-line task "Paste to prompt"

**Files:**
- Modify: `src/main.rs` (the `runpanel::TaskAction::Paste` arm in `populate_tasks`)

- [ ] **Step 1: Wrap multi-line commands in bracketed-paste markers**

Find:

```rust
                    runpanel::TaskAction::Paste => {
                        if let Some(term) = focused_terminal(&win_r, &nb_r) {
                            term.feed_child(task.cmd.as_bytes());
                            term.grab_focus();
                        }
                    }
```

Replace with:

```rust
                    runpanel::TaskAction::Paste => {
                        if let Some(term) = focused_terminal(&win_r, &nb_r) {
                            if task.cmd.contains('\n') {
                                // Multi-line: wrap in bracketed-paste so the
                                // shell treats embedded newlines as pasted text
                                // instead of executing each line immediately.
                                let mut bytes = Vec::with_capacity(task.cmd.len() + 12);
                                bytes.extend_from_slice(b"\x1b[200~");
                                bytes.extend_from_slice(task.cmd.as_bytes());
                                bytes.extend_from_slice(b"\x1b[201~");
                                term.feed_child(&bytes);
                            } else {
                                term.feed_child(task.cmd.as_bytes());
                            }
                            term.grab_focus();
                        }
                    }
```

- [ ] **Step 2: Verify build**

Run: `cargo check`
Expected: compiles.

- [ ] **Step 3: Manual verification**

Define a task with a multi-line `cmd` (TOML triple-quoted string). Click →; the lines land at the prompt as a single editable paste (not auto-run). A single-line task behaves exactly as before.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: bracketed-paste multi-line task commands so they don't auto-run"
```

---

## Task 21: Move the control socket to $XDG_RUNTIME_DIR (with fallback)

**Files:**
- Modify: `src/ipc.rs`, `src/bin/sidekick-ctl.rs`, `src/bin/sidekick-hook.rs`, `README.md`

- [ ] **Step 1: Update `ipc::socket_path`**

In `src/ipc.rs` find:

```rust
pub fn socket_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".local/run/sidekick.sock")
}
```

Replace with:

```rust
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
```

(The existing `start()` already `create_dir_all`s the parent and chmods it 0700, so the new `sidekick/` subdir is created correctly.)

- [ ] **Step 2: Update `sidekick-ctl`**

In `src/bin/sidekick-ctl.rs` add this function above `fn main()`:

```rust
fn socket_path() -> std::path::PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime.is_empty() {
            return std::path::PathBuf::from(runtime).join("sidekick/sidekick.sock");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".local/run/sidekick.sock")
}
```

Then find:

```rust
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let socket = format!("{home}/.local/run/sidekick.sock");

    let mut stream = UnixStream::connect(&socket).unwrap_or_else(|e| {
```

Replace with:

```rust
    let socket = socket_path();
    let socket = socket.to_string_lossy().to_string();

    let mut stream = UnixStream::connect(&socket).unwrap_or_else(|e| {
```

- [ ] **Step 3: Update `sidekick-hook`**

In `src/bin/sidekick-hook.rs` find:

```rust
fn socket_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".local/run/sidekick.sock")
}
```

Replace with:

```rust
fn socket_path() -> std::path::PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime.is_empty() {
            return std::path::PathBuf::from(runtime).join("sidekick/sidekick.sock");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".local/run/sidekick.sock")
}
```

- [ ] **Step 4: Update the README**

In `README.md`, "Command-Line Control" section, replace the socket path block/sentence so it reads:

```
`$XDG_RUNTIME_DIR/sidekick/sidekick.sock` (falling back to
`~/.local/run/sidekick.sock` when `XDG_RUNTIME_DIR` is unset).
```

- [ ] **Step 5: Verify build**

Run: `cargo build --bins`
Expected: all four binaries compile.

- [ ] **Step 6: Manual verification**

Launch `sidekick`, then `./target/debug/sidekick-ctl ping`.
Expected: `{"ok":true}`. Confirm the socket exists under `$XDG_RUNTIME_DIR/sidekick/`.

- [ ] **Step 7: Commit**

```bash
git add src/ipc.rs src/bin/sidekick-ctl.rs src/bin/sidekick-hook.rs README.md
git commit -m "refactor: place control socket under XDG_RUNTIME_DIR with ~/.local/run fallback"
```

---

## Task 22: Persist custom tab names and split ratios across session restore

**Problem:** Session restore drops custom tab names and resets all splits to 50/50.

**Files:**
- Modify: `src/session.rs` (format), `src/main.rs` (rename plumbing, snapshot, restore)

> **Execution note:** this is the largest task and touches every `add_tab` / `add_tab_with_command` call site. Do it last and lean on the compiler to enumerate the call sites.

### 22a: Extend the session format

- [ ] **Step 1: Replace the roundtrip test**

In `src/session.rs` replace the existing `session_roundtrips` test with:

```rust
    #[test]
    fn session_roundtrips() {
        let session = Session {
            tabs: vec![
                TabLayout {
                    name: Some("build".into()),
                    root: Node::Terminal { cwd: "/tmp".into() },
                },
                TabLayout {
                    name: None,
                    root: Node::Split {
                        orientation: "h".into(),
                        ratio: Some(0.4),
                        first: Box::new(Node::Terminal { cwd: "/a".into() }),
                        second: Box::new(Node::Terminal { cwd: "/b".into() }),
                    },
                },
            ],
        };
        let json = serde_json::to_string(&session).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tabs.len(), 2);
        assert_eq!(back.tabs[0].name.as_deref(), Some("build"));
        assert_eq!(back.tabs[0].root.first_cwd(), "/tmp");
        match &back.tabs[1].root {
            Node::Split { ratio, second, .. } => {
                assert_eq!(*ratio, Some(0.4));
                assert_eq!(second.first_cwd(), "/b");
            }
            _ => panic!("expected split"),
        }
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --bin sidekick session_roundtrips`
Expected: FAIL (`TabLayout` undefined, `Split` has no `ratio`).

- [ ] **Step 3: Update the types**

Find:

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    Terminal {
        cwd: String,
    },
    Split {
        /// "h" = side by side, "v" = stacked.
        orientation: String,
        first: Box<Node>,
        second: Box<Node>,
    },
}
```

Replace with:

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    Terminal {
        cwd: String,
    },
    Split {
        /// "h" = side by side, "v" = stacked.
        orientation: String,
        /// Divider position as a fraction (0.0–1.0) of the split's size.
        #[serde(default)]
        ratio: Option<f64>,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// One restored tab: an optional custom name plus its terminal/split layout.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TabLayout {
    #[serde(default)]
    pub name: Option<String>,
    pub root: Node,
}
```

Find:

```rust
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Session {
    #[serde(default)]
    pub tabs: Vec<Node>,
}
```

Replace with:

```rust
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Session {
    #[serde(default)]
    pub tabs: Vec<TabLayout>,
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --bin sidekick session_roundtrips`
Expected: PASS. (Old on-disk sessions fail to deserialize and yield a fresh start — acceptable, sessions are disposable.)

### 22b: Track custom tab names by tab id

- [ ] **Step 5: Add a shared tab-name map**

In `build_ui`, near the `agent_map` creation, add:

```rust
    // tab id -> custom tab name (for session persistence).
    let tab_names: Rc<RefCell<HashMap<u64, String>>> = Rc::new(RefCell::new(HashMap::new()));
```

- [ ] **Step 6: Thread `tab_names` into tab creation**

Find:

```rust
fn add_tab(notebook: &Notebook, cfg: &config::Config, cwd: Option<&str>, agent_map: &AgentMap) {
    add_tab_with_command(notebook, cfg, cwd, agent_map, None);
}
```

Replace with:

```rust
fn add_tab(
    notebook: &Notebook,
    cfg: &config::Config,
    cwd: Option<&str>,
    agent_map: &AgentMap,
    tab_names: &Rc<RefCell<HashMap<u64, String>>>,
) {
    add_tab_with_command(notebook, cfg, cwd, agent_map, tab_names, None);
}
```

Find the `add_tab_with_command` signature:

```rust
fn add_tab_with_command(
    notebook: &Notebook,
    cfg: &config::Config,
    cwd: Option<&str>,
    agent_map: &AgentMap,
    startup_command: Option<String>,
) {
```

Replace with:

```rust
fn add_tab_with_command(
    notebook: &Notebook,
    cfg: &config::Config,
    cwd: Option<&str>,
    agent_map: &AgentMap,
    tab_names: &Rc<RefCell<HashMap<u64, String>>>,
    startup_command: Option<String>,
) {
```

- [ ] **Step 7: Initialize the custom-title cell from the map; render it in the tick**

In `add_tab_with_command`, find:

```rust
    let custom_title: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
```

Replace with:

```rust
    let custom_title: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(tab_names.borrow().get(&tab_id).cloned()));
```

Where the 500 ms label tick captures its `*_ref` clones (just before `glib::timeout_add_local(Duration::from_millis(500), ...)`), add:

```rust
        let tab_names_tick = Rc::clone(tab_names);
```

In that tick, find:

```rust
            let (auto_title, detail_text) = tab::tab_title_parts(pid);
            let title_text = custom_title_ref.borrow().clone().unwrap_or(auto_title);
```

Replace with:

```rust
            let (auto_title, detail_text) = tab::tab_title_parts(pid);
            let title_text = custom_title_ref
                .borrow()
                .clone()
                .or_else(|| tab_names_tick.borrow().get(&tab_id).cloned())
                .unwrap_or(auto_title);
```

- [ ] **Step 8: Pass `tab_names` + `tab_id` into the rename gesture**

Find:

```rust
    {
        let custom = Rc::clone(&custom_title);
        let label_w: gtk4::Widget = tab_label.clone().upcast();
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |gesture, _n, x, y| {
            show_tab_context_menu(&label_w, x, y, Rc::clone(&custom));
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        tab_label.add_controller(gesture);
    }
```

Replace with:

```rust
    {
        let custom = Rc::clone(&custom_title);
        let label_w: gtk4::Widget = tab_label.clone().upcast();
        let names = Rc::clone(tab_names);
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |gesture, _n, x, y| {
            show_tab_context_menu(&label_w, x, y, Rc::clone(&custom), Rc::clone(&names), tab_id);
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        tab_label.add_controller(gesture);
    }
```

- [ ] **Step 9: Update `show_tab_context_menu` and `prompt_tab_rename`**

Find:

```rust
fn show_tab_context_menu(
    parent: &gtk4::Widget,
    x: f64,
    y: f64,
    custom_title: Rc<RefCell<Option<String>>>,
) {
```

Replace with:

```rust
fn show_tab_context_menu(
    parent: &gtk4::Widget,
    x: f64,
    y: f64,
    custom_title: Rc<RefCell<Option<String>>>,
    tab_names: Rc<RefCell<HashMap<u64, String>>>,
    tab_id: u64,
) {
```

Inside it, find:

```rust
    add_filetree_menu_item(&vbox, "Rename tab…", &popover, {
        let parent_w = parent.clone();
        let custom = Rc::clone(&custom_title);
        move || prompt_tab_rename(&parent_w, Rc::clone(&custom))
    });
    if custom_title.borrow().is_some() {
        add_filetree_menu_item(&vbox, "Reset name", &popover, {
            let custom = Rc::clone(&custom_title);
            move || {
                *custom.borrow_mut() = None;
            }
        });
    }
```

Replace with:

```rust
    add_filetree_menu_item(&vbox, "Rename tab…", &popover, {
        let parent_w = parent.clone();
        let custom = Rc::clone(&custom_title);
        let names = Rc::clone(&tab_names);
        move || prompt_tab_rename(&parent_w, Rc::clone(&custom), Rc::clone(&names), tab_id)
    });
    if custom_title.borrow().is_some() {
        add_filetree_menu_item(&vbox, "Reset name", &popover, {
            let custom = Rc::clone(&custom_title);
            let names = Rc::clone(&tab_names);
            move || {
                *custom.borrow_mut() = None;
                names.borrow_mut().remove(&tab_id);
            }
        });
    }
```

Find:

```rust
fn prompt_tab_rename(parent: &gtk4::Widget, custom_title: Rc<RefCell<Option<String>>>) {
```

Replace with:

```rust
fn prompt_tab_rename(
    parent: &gtk4::Widget,
    custom_title: Rc<RefCell<Option<String>>>,
    tab_names: Rc<RefCell<HashMap<u64, String>>>,
    tab_id: u64,
) {
```

Inside `prompt_tab_rename`, find:

```rust
        entry.connect_activate(move |e| {
            let text = e.text().trim().to_string();
            *custom_title.borrow_mut() = if text.is_empty() { None } else { Some(text) };
            win_c.close();
        });
```

Replace with:

```rust
        entry.connect_activate(move |e| {
            let text = e.text().trim().to_string();
            if text.is_empty() {
                *custom_title.borrow_mut() = None;
                tab_names.borrow_mut().remove(&tab_id);
            } else {
                *custom_title.borrow_mut() = Some(text.clone());
                tab_names.borrow_mut().insert(tab_id, text);
            }
            win_c.close();
        });
```

- [ ] **Step 10: Update every `add_tab` / `add_tab_with_command` call site**

Add a `tab_names` field to `PaletteContext`:

```rust
    tab_names: &'a Rc<RefCell<HashMap<u64, String>>>,
```

…and set `tab_names: &tab_names,` in the `build_palette_actions(PaletteContext { ... })` call.

Then update each call site (capture a `Rc::clone(&tab_names)` into the enclosing closure block, mirroring how `agent_map` is already cloned as `agent_map_kb`, `agent_map_ipc`, etc., and pass `&that_clone`):

- `build_ui` initial: `add_tab(&notebook, &cfg.borrow(), initial_dir, &agent_map, &tab_names)` (see Step 15 for `restore_session`).
- Hosts panel closure: capture `let tab_names_h = Rc::clone(&tab_names);`, pass `&tab_names_h` to `add_tab_with_command`.
- Key handler "New tab": capture `let tab_names_kb = Rc::clone(&tab_names);` into the key controller block, pass `&tab_names_kb`.
- IPC `NewTab`: capture `let tab_names_ipc = Rc::clone(&tab_names);` into the IPC dispatch block, pass `&tab_names_ipc`.
- Palette "New Tab" action: capture `let tab_names = Rc::clone(ctx.tab_names);`, pass `&tab_names`.
- `open_config_in_nvim`, `open_path_in_nvim`, `open_file_from_file_manager`: add a `tab_names: &Rc<RefCell<HashMap<u64, String>>>` parameter and forward it to `add_tab_with_command`. Update their callers (tree row-activated handler, `Ctrl+,` handler, palette "Open Config File" action) to capture and pass a `Rc::clone(&tab_names)`.

Mechanically: wherever the compiler reports a missing argument, capture and pass `&tab_names` (or a clone).

- [ ] **Step 11: Verify build**

Run: `cargo check`
Expected: compiles after every call site is updated.

### 22c: Snapshot and restore names + ratios

- [ ] **Step 12: Snapshot tab names and split ratios**

Find:

```rust
fn snapshot_session(notebook: &Notebook) -> session::Session {
    let mut tabs = Vec::new();
    for i in 0..notebook.n_pages() {
        if let Some(page) = notebook.nth_page(Some(i)) {
            if let Some(node) = capture_layout(&page) {
                tabs.push(node);
            }
        }
    }
    session::Session { tabs }
}
```

Replace with:

```rust
fn snapshot_session(
    notebook: &Notebook,
    agent_map: &AgentMap,
    tab_names: &Rc<RefCell<HashMap<u64, String>>>,
) -> session::Session {
    let mut tabs = Vec::new();
    for i in 0..notebook.n_pages() {
        if let Some(page) = notebook.nth_page(Some(i)) {
            if let Some(root) = capture_layout(&page) {
                // The tab's name is keyed by the tab id of its first terminal.
                let name = pane::collect_terminals_pub(&page)
                    .into_iter()
                    .next()
                    .and_then(|term| {
                        let key = term.as_ptr() as usize;
                        agent_map.borrow().get(&key).map(|(id, _)| *id)
                    })
                    .and_then(|id| tab_names.borrow().get(&id).cloned());
                tabs.push(session::TabLayout { name, root });
            }
        }
    }
    session::Session { tabs }
}
```

Find the `capture_layout` Split branch:

```rust
    if let Ok(paned) = widget.clone().downcast::<gtk4::Paned>() {
        let first = paned.start_child().and_then(|w| capture_layout(&w));
        let second = paned.end_child().and_then(|w| capture_layout(&w));
        return match (first, second) {
            (Some(a), Some(b)) => Some(session::Node::Split {
                orientation: if paned.orientation() == gtk4::Orientation::Vertical {
                    "v".to_string()
                } else {
                    "h".to_string()
                },
                first: Box::new(a),
                second: Box::new(b),
            }),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        };
    }
```

Replace with:

```rust
    if let Ok(paned) = widget.clone().downcast::<gtk4::Paned>() {
        let first = paned.start_child().and_then(|w| capture_layout(&w));
        let second = paned.end_child().and_then(|w| capture_layout(&w));
        let size = match paned.orientation() {
            gtk4::Orientation::Vertical => paned.height(),
            _ => paned.width(),
        };
        let ratio = if size > 0 {
            Some(paned.position() as f64 / size as f64)
        } else {
            None
        };
        return match (first, second) {
            (Some(a), Some(b)) => Some(session::Node::Split {
                orientation: if paned.orientation() == gtk4::Orientation::Vertical {
                    "v".to_string()
                } else {
                    "h".to_string()
                },
                ratio,
                first: Box::new(a),
                second: Box::new(b),
            }),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        };
    }
```

- [ ] **Step 13: Update the two `snapshot_session` call sites**

Close-request handler, find:

```rust
        let nb = notebook.clone();
        window.connect_close_request(move |_| {
            session::save(&snapshot_session(&nb));
            glib::Propagation::Proceed
        });
```

Replace with:

```rust
        let nb = notebook.clone();
        let agent_map_s = Rc::clone(&agent_map);
        let tab_names_s = Rc::clone(&tab_names);
        window.connect_close_request(move |_| {
            session::save(&snapshot_session(&nb, &agent_map_s, &tab_names_s));
            glib::Propagation::Proceed
        });
```

60-second timer, find:

```rust
        let nb = notebook.clone();
        glib::timeout_add_seconds_local(60, move || {
            session::save(&snapshot_session(&nb));
            glib::ControlFlow::Continue
        });
```

Replace with:

```rust
        let nb = notebook.clone();
        let agent_map_s = Rc::clone(&agent_map);
        let tab_names_s = Rc::clone(&tab_names);
        glib::timeout_add_seconds_local(60, move || {
            session::save(&snapshot_session(&nb, &agent_map_s, &tab_names_s));
            glib::ControlFlow::Continue
        });
```

- [ ] **Step 14: Restore names and ratios**

Find:

```rust
fn restore_session(notebook: &Notebook, cfg: &config::Config, agent_map: &AgentMap) -> bool {
    let Some(saved) = session::load() else {
        return false;
    };
    if saved.tabs.is_empty() {
        return false;
    }
    for node in &saved.tabs {
        add_tab(notebook, cfg, Some(node.first_cwd()), agent_map);
        // The tab's root terminal is the page we just appended.
        let Some(term) = notebook
            .nth_page(Some(notebook.n_pages() - 1))
            .and_then(|p| p.downcast::<vte4::Terminal>().ok())
        else {
            continue;
        };
        expand_layout(notebook, cfg, agent_map, &term, node);
    }
    notebook.set_current_page(Some(0));
    true
}
```

Replace with:

```rust
fn restore_session(
    notebook: &Notebook,
    cfg: &config::Config,
    agent_map: &AgentMap,
    tab_names: &Rc<RefCell<HashMap<u64, String>>>,
) -> bool {
    let Some(saved) = session::load() else {
        return false;
    };
    if saved.tabs.is_empty() {
        return false;
    }
    for tab in &saved.tabs {
        add_tab(notebook, cfg, Some(tab.root.first_cwd()), agent_map, tab_names);
        // The tab's root terminal is the page we just appended.
        let Some(term) = notebook
            .nth_page(Some(notebook.n_pages() - 1))
            .and_then(|p| p.downcast::<vte4::Terminal>().ok())
        else {
            continue;
        };
        // Record the custom name against this tab's id so it shows (next tick)
        // and persists on the next snapshot.
        if let Some(name) = &tab.name {
            let key = term.as_ptr() as usize;
            if let Some((id, _)) = agent_map.borrow().get(&key) {
                tab_names.borrow_mut().insert(*id, name.clone());
            }
        }
        expand_layout(notebook, cfg, agent_map, &term, &tab.root);
    }
    notebook.set_current_page(Some(0));
    true
}
```

- [ ] **Step 15: Update the `restore_session` call site**

Find:

```rust
    let restored = initial_dir.is_none()
        && cfg.borrow().behavior.restore_session
        && restore_session(&notebook, &cfg.borrow(), &agent_map);
    if !restored {
        add_tab(&notebook, &cfg.borrow(), initial_dir, &agent_map);
    }
```

Replace with:

```rust
    let restored = initial_dir.is_none()
        && cfg.borrow().behavior.restore_session
        && restore_session(&notebook, &cfg.borrow(), &agent_map, &tab_names);
    if !restored {
        add_tab(&notebook, &cfg.borrow(), initial_dir, &agent_map, &tab_names);
    }
```

- [ ] **Step 16: Apply restored split ratios in `expand_layout`**

Find:

```rust
    let session::Node::Split {
        orientation,
        first,
        second,
    } = node
    else {
        return;
    };
    let orient = if orientation == "v" {
        gtk4::Orientation::Vertical
    } else {
        gtk4::Orientation::Horizontal
    };
    let new_term = split_terminal(
        notebook,
        cfg,
        agent_map,
        term,
        orient,
        Some(second.first_cwd()),
        None,
        None,
    );
    expand_layout(notebook, cfg, agent_map, term, first);
    expand_layout(notebook, cfg, agent_map, &new_term, second);
```

Replace with:

```rust
    let session::Node::Split {
        orientation,
        ratio,
        first,
        second,
    } = node
    else {
        return;
    };
    let orient = if orientation == "v" {
        gtk4::Orientation::Vertical
    } else {
        gtk4::Orientation::Horizontal
    };
    let new_term = split_terminal(
        notebook,
        cfg,
        agent_map,
        term,
        orient,
        Some(second.first_cwd()),
        None,
        None,
    );
    // Apply the saved divider ratio once layout has settled.
    if let Some(r) = ratio {
        if let Some(paned) = new_term
            .parent()
            .and_then(|p| p.downcast::<gtk4::Paned>().ok())
        {
            let r = *r;
            let paned_c = paned.clone();
            glib::idle_add_local(move || {
                let size = match paned_c.orientation() {
                    gtk4::Orientation::Vertical => paned_c.height(),
                    _ => paned_c.width(),
                };
                if size > 0 {
                    paned_c.set_position((r * size as f64).round() as i32);
                }
                glib::ControlFlow::Break
            });
        }
    }
    expand_layout(notebook, cfg, agent_map, term, first);
    expand_layout(notebook, cfg, agent_map, &new_term, second);
```

- [ ] **Step 17: Verify build and tests**

Run: `cargo fmt && cargo test && cargo check`
Expected: compiles, all tests pass.

- [ ] **Step 18: Manual verification**

Open two tabs, rename one (right-click its tab label → Rename tab…), split a tab and drag the divider to ~30/70. Close the window, relaunch. The renamed tab shows its name; the split restores at roughly the saved ratio.

- [ ] **Step 19: Commit**

```bash
git add src/session.rs src/main.rs
git commit -m "feat: persist custom tab names and split ratios across session restore"
```

---

## Task 23: Documentation — llm task field and project-task trust note

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the `llm` task field**

In the "Run Panel Tasks" section, after the existing `[[tasks]]` example, add a paragraph and example explaining that a task may carry an `llm` prompt, which shows a `✦` button that copies the prompt to the clipboard:

```toml
[[tasks]]
name = "explain failure"
cmd  = "cargo test"
llm  = "Explain the test failure above and propose a fix."
```

- [ ] **Step 2: Add a project-task trust note**

In the same section, add:

```markdown
**Trust note:** project tasks come from a `.sidekick.toml` committed to the
repository you are in. Treat them like any other code in that repo — running a
task (`▶`) executes its command, and `open_browser` loads its URL in the
embedded browser. Only run project tasks from repositories you trust, the same
way you would with VS Code workspace tasks.
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document the llm task field and project-task trust"
```

---

## Final verification

- [ ] **Step 1: Full check**

Run: `cargo fmt --check && cargo test && cargo check && cargo build --bins`
Expected: formatted, all tests pass, all four binaries build.

- [ ] **Step 2: Smoke test**

Run `cargo run --bin sidekick` and exercise: new tab, split, rename + restart (names/ratios persist), git discard confirm, quick-open with `(`, Ctrl+1..9, run a `sleep` task and stop it via the status dot, agents panel WAIT ordering, `sidekick-ctl ping`.

---

## Self-review notes

- **Spec coverage:** every review item is covered — Bugs (Tasks 2,3,4,5,6,8,9,10,11,1; `tree_busy` = 15; name/ratio persistence = 22), Security (Tasks 13, 23), Performance (Tasks 12, 7, 14), Bloat (theme: dropped per user; `llm`: documented in 23; hosts: no change), Features (Tasks 16, 17, 18, 22, 19, plus 20 + 21 for the smaller items), Browser UTF-8 = Task 9.
- **Type consistency:** `changed_files(root)`/`ahead_count(root)` updated at all call sites (Task 7); `TabLayout`/`Node::Split{ratio}` used consistently (Task 22); `notify_*`/`wire_agent_state_handlers` signatures updated at all call sites (Task 17); `tab_names` threaded through every `add_tab`/`add_tab_with_command` caller (Task 22 Step 10).
- **Riskiest task: 22** (many call sites + format change). Execute last, lean on the compiler.
