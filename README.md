# sidekick

sidekick is a native Linux terminal workspace inspired by
[cmux](https://cmux.com). It keeps the fast terminal-first workflow, but replaces
cmux's macOS-only vertical tab UI with a Linux desktop app that includes a file
tree, lightweight file editing, syntax highlighting, and git diffs.

Think of it as a small IDE wrapped around your shell: terminals stay at the
center, while the sidebar tracks the active working directory and gives you just
enough project context to inspect, edit, and review changes without leaving the
window.

## Features

- Native GTK4/VTE terminal tabs.
- Terminal pane splits, both side-by-side and stacked.
- File tree sidebar that follows the focused terminal's current directory.
- Built-in editor tabs powered by GtkSourceView, with syntax highlighting, line
  numbers, bracket matching, auto-indent, and `Ctrl+S` saves.
- Git changes panel for the active repository, split into conflicted, staged,
  and unstaged files, with ahead/behind counts on the push and pull buttons.
- Read-only colored diff tabs for changed files; conflicted files open a
  marker-highlighted view with ours/base/theirs sections tinted.
- Optional embedded browser panel for quick docs/searches.
- Configurable terminal font, cursor, padding, scrollback, hyperlink behavior,
  bell, and opacity.
- Shell integration for background tab notification dots.
- Find in scrollback, app-wide font zoom, drag-reorder and renameable tabs.
- Session restore: tabs, split layouts, and working directories come back on
  launch.
- Desktop notifications and an activity-bar badge when agents are waiting.
- Agents dashboard panel: every tab's agent state with elapsed time; click a
  row to jump to that tab. `Ctrl+Shift+J` jumps to the next tab whose agent
  wants attention.
- Hosts panel listing `~/.ssh/config` entries and, opt-in via
  `[hosts] show_teleport`, Teleport (`tsh`) nodes — including Beam instances
  by alias; activating a host opens a connected tab.
- Command palette (`Ctrl+Shift+P`) and a keyboard shortcuts help window
  (`Ctrl+Shift+?`).
- Desktop notification when a command running 15s+ finishes while the window
  is unfocused, with exit status and duration (requires shell integration).
- Run-panel tasks execute in a dedicated split with a live status indicator,
  optionally opening the embedded browser (inspector enabled) on a URL.
- Editor follows external file changes (agents editing under you), warns
  before overwriting newer on-disk content, and saves atomically.
- Local Unix-socket control commands via `sidekick-ctl`.
- Optional `sidekick-hook` for showing Claude Code edit diffs inside
  sidekick before accepting or rejecting them.

## Status

sidekick is early open source software. It is useful as a daily local tool,
but it is not yet packaged for distributions and the UI surface is intentionally
small. Expect rough edges, especially outside Linux desktop environments with
recent GTK and WebKitGTK packages.

The project is currently developed and tested on Arch Linux. Other Linux
distributions should work as long as they provide compatible GTK4, VTE4,
GtkSourceView 5, and WebKitGTK 6 packages.

## Requirements

- Rust stable toolchain
- GTK4
- VTE4
- GtkSourceView 5
- WebKitGTK 6
- Git, for branch labels and the git changes panel

On Arch Linux:

```bash
sudo pacman -S rust gtk4 vte4 gtksourceview5 webkitgtk-6.0 git
```

Package names differ by distribution. On Debian/Ubuntu-style systems, look for
the development packages for `gtk4`, `vte-2.91-gtk4`, `gtksourceview-5`, and
`webkitgtk-6.0`.

## Build And Run

```bash
git clone https://github.com/<your-user-or-org>/sidekick-term.git
cd sidekick-term
cargo run --bin sidekick
```

For an optimized local build:

```bash
cargo build --release
./target/release/sidekick
```

## Install

To install the three binaries into `~/.local/bin`:

```bash
cargo install --path . --root ~/.local
```

This installs:

- `sidekick`: the main desktop application.
- `sidekick-ctl`: a small command-line client for talking to a running
  sidekick instance.
- `sidekick-hook`: an optional Claude Code hook that can present proposed file edits
  as accept/reject diffs inside sidekick.

Make sure `~/.local/bin` is on your `PATH`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

To add a desktop launcher for app menus:

```bash
install -Dm644 icon.png ~/.local/share/icons/hicolor/512x512/apps/sidekick.png
install -Dm644 com.travismedia.sidekick.desktop ~/.local/share/applications/com.travismedia.sidekick.desktop
```

## Usage

Start the app:

```bash
sidekick
```

Each terminal tab launches your `$SHELL`. The sidebar updates from the focused
terminal's current working directory, so `cd` into a project and sidekick
will show that directory's file tree and git changes.

Open files by activating them in the file tree. Directories expand in place.
Changed git files open as read-only diff tabs when activated from the git panel.
Conflicted files (a merge/rebase gone sideways) appear in a CONFLICTS section:
activating one opens the working-tree file with the `<<<<<<<`/`=======`/`>>>>>>>`
markers highlighted, and right-click → "Mark resolved (stage)" stages it once
fixed. The push and pull buttons show how many commits the branch is ahead or
behind its upstream.
File-tree files can open either in the built-in editor or in a new `nvim`
terminal tab, based on the `[editor]` configuration.

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+C`, `Ctrl+Insert` | Copy selected terminal text |
| `Ctrl+V` | Paste a clipboard image as a temp PNG path; with a text-only clipboard the key reaches the shell (verbatim insert) |
| `Ctrl+Shift+V`, `Shift+Insert` | Paste text into the focused terminal |
| `Ctrl+Shift+T` | New terminal tab |
| `Ctrl+Shift+W` | Close the current pane, editor tab, or diff tab; the final tab stays open |
| `Ctrl+Tab` | Next tab |
| `Ctrl+Shift+Tab` | Previous tab |
| `Ctrl+1` … `Ctrl+9` | Jump to tab 1–9 |
| `Ctrl+Shift+J` | Jump to next tab whose agent wants attention (waiting first, then done, then running) |
| `Ctrl+Shift+D` | Split terminal right |
| `Ctrl+Shift+X` | Split terminal down |
| `Alt+Left` | Focus previous terminal pane |
| `Alt+Right` | Focus next terminal pane |
| `Ctrl+Shift+H` | Find in scrollback (Enter: next match up, Shift+Enter: down, Esc: close) |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Font zoom in / out / reset (all terminals) |
| `Ctrl+Shift+E` | Show file explorer panel |
| `Ctrl+Shift+G` | Show git panel |
| `Ctrl+F` | Quick open: search file names |
| `Ctrl+Shift+F` | Show search-in-files panel |
| `Ctrl+Shift+R` | Show run panel |
| `Ctrl+Shift+A` | Show agents dashboard panel |
| `Ctrl+Shift+P` | Command palette |
| `Ctrl+Shift+?` | Keyboard shortcuts help |
| `Ctrl+Shift+B` | Toggle sidebar |
| `Ctrl+Shift+O` | Toggle embedded browser panel |
| `Ctrl+,` | Open sidekick config in `nvim` |
| `Ctrl+S` | Save the current editor tab |

Tabs can be drag-reordered. Right-click a terminal tab label to rename it
(an empty name resets to the automatic cwd-based title).

## Configuration

On first run, sidekick creates:

```text
~/.config/sidekick/config.toml
```

Default configuration:

```toml
[theme]
# Available themes: catppuccin-mocha
name = "catppuccin-mocha"
# Background opacity: 0.0 is transparent, 1.0 is opaque
opacity = 0.9

[font]
family = "Monospace"
size = 15
bold_is_bright = true

[cursor]
# block | ibeam | underline
shape = "block"
blink = true

[window]
padding = 8

[behavior]
# Use -1 for unlimited scrollback
scrollback_lines = 10000
scroll_on_output = false
scroll_on_keystroke = true
allow_hyperlinks = true
mouse_autohide = true
audible_bell = false
# Reopen tabs (and their split layout / directories) from the previous session
restore_session = true

[editor]
# builtin | nvim
file_manager_open = "builtin"
word_wrap = true

[hosts]
# Show Teleport nodes (`tsh ls`) in the Hosts panel. When false, tsh is
# never invoked at all.
show_teleport = false
```

The session is saved to `~/.local/state/sidekick/session.json` when the
window closes (and every minute as crash protection). Exiting every shell
clears the session; launching with `--dir` skips restore for that run.

## Run Panel Tasks

Tasks come from `[[tasks]]` in the global config and from a `.sidekick.toml`
in the project root. The `→` button types the command into the focused
terminal; `▶` runs it in a dedicated split below with a live running/done
indicator, so agent prompts stay clean. A task can also open the embedded
browser when it starts:

```toml
[[tasks]]
name = "dev server"
cmd  = "npm run dev"
open_browser = "http://localhost:3000"
```

The browser panel has the WebKit inspector enabled — right-click →
"Inspect Element" while previewing whatever your agents are building.

While a `▶` task is running in its split, its run-panel status shows a yellow
dot; click the dot to stop the task (sends `SIGTERM` to its process group).

A task may also carry an `llm` prompt. When set, the task row shows a `✦`
button that copies that prompt to the clipboard — handy for pasting context
into an agent:

```toml
[[tasks]]
name = "explain failure"
cmd  = "cargo test"
llm  = "Explain the test failure above and propose a fix."
```

**Trust note:** project tasks come from a `.sidekick.toml` committed to the
repository you are in. Treat them like any other code in that repo — running a
task (`▶`) executes its command, and `open_browser` loads its URL in the
embedded browser. Only run project tasks from repositories you trust, the same
way you would with VS Code workspace tasks.

When an agent flips to **waiting** or **finished** while the sidekick window
is unfocused, a desktop notification is sent (clicking it focuses that tab),
and the activity bar shows a badge with the number of agents currently waiting
for input.

## Shell Integration

Shell integration enables the red notification dot on background tabs when a
command finishes and the shell returns to the prompt. It also provides an agent
status helper for tab dots:

```bash
sidekick_agent_status busy   # yellow: agent is working
sidekick_agent_status ready  # green: agent is waiting on user input/approval
sidekick_agent_status done   # blue: agent finished
sidekick_agent_status idle   # clear the agent dot
```

Agent hooks can use `sidekick-agent-status` for the same statuses. The command
writes directly to the current terminal, so it works even when hook stdout is
captured by the agent.

From a fresh clone, install the status helper and merge Claude/Codex hook config:

```bash
scripts/install-agent-status-hooks
```

The installer builds `sidekick-agent-status` and `sidekick-hook`, installs both
to `~/.local/bin`, and adds hooks to `~/.claude/settings.json` and
`~/.codex/config.toml`. For Claude Code that includes `PreToolUse` → busy (an
approved tool flips the dot back from green to yellow) and `SessionEnd` → idle
(closing a session clears its tab from the agents panel). The `sidekick-hook`
edit-review hook (accept/reject diff tabs for every agent file edit) is opt-in:
pass `--edit-review` to wire it with a `Write|Edit|MultiEdit` matcher. When the
Pi coding agent is detected at `~/.pi/agent`, a status extension is installed to
`~/.pi/agent/extensions/sidekick-status.ts`. The installer is idempotent — safe
to re-run; existing hooks are kept. Restart any open Claude Code, Codex, or Pi
sessions after running it. The exact config snippets are also available in
`examples/claude/settings.json` and `examples/codex/config.toml`.

For zsh:

```bash
mkdir -p ~/.config/sidekick
cp shell-integration.zsh ~/.config/sidekick/
echo 'source ~/.config/sidekick/shell-integration.zsh' >> ~/.zshrc
```

Restart your shell or source `~/.zshrc`. Without this integration, sidekick still
works, but background tabs will not show command-completion dots and
long-command notifications will not fire.

If you installed shell integration before the exit-status feature was added,
re-copy `shell-integration.zsh` to pick it up.

## Command-Line Control

`sidekick` listens on:

```text
$XDG_RUNTIME_DIR/sidekick/sidekick.sock
```

(falling back to `~/.local/run/sidekick.sock` when `XDG_RUNTIME_DIR` is unset).

You can check that a running instance is reachable:

```bash
sidekick-ctl ping
```

Open a new tab in the running instance:

```bash
sidekick-ctl new-tab
```

Agent status commands (`agent-busy`, `agent-ready`, `agent-done`,
`agent-idle`) read `SIDEKICK_TAB_ID` from the environment. sidekick exports
this variable into every shell it spawns, so a hook or script run inside a
sidekick terminal updates that terminal's tab indicator — not whichever tab
happens to be focused. Outside a sidekick shell the variable is absent and
the update falls back to the focused terminal.

## Claude Code Hook

`sidekick-hook` is optional. It is designed for Claude Code's `PreToolUse` hook and
can show proposed `Write`, `Edit`, and `MultiEdit` changes as diffs inside sidekick.
Accepting the diff lets the edit proceed; rejecting exits with code `2`.

`scripts/install-agent-status-hooks --edit-review` registers it (a `PreToolUse`
hook with a `Write|Edit|MultiEdit` matcher in `~/.claude/settings.json`); without
the flag the binary is installed but not wired. To wire it manually instead:

```bash
mkdir -p ~/.claude/hooks/PreToolUse
cp ~/.local/bin/sidekick-hook ~/.claude/hooks/PreToolUse/sidekick-hook
chmod +x ~/.claude/hooks/PreToolUse/sidekick-hook
```

If sidekick is not running, the hook allows edits to proceed normally.

Claude Code can also drive tab status dots using hooks. Add these hooks alongside
any existing hooks:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "sidekick-agent-status busy" }
        ]
      }
    ],
    "PermissionRequest": [
      {
        "hooks": [
          { "type": "command", "command": "sidekick-agent-status ready" }
        ]
      }
    ],
    "PreToolUse": [
      {
        "hooks": [
          { "type": "command", "command": "sidekick-agent-status busy" }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "sidekick-agent-status done" }
        ]
      }
    ],
    "SessionEnd": [
      {
        "hooks": [
          { "type": "command", "command": "sidekick-agent-status idle" }
        ]
      }
    ]
  }
}
```

For Codex CLI, enable hooks and add the same status bridge:

```toml
[features]
hooks = true

[[hooks.UserPromptSubmit]]
[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "sidekick-agent-status busy"

[[hooks.PermissionRequest]]
[[hooks.PermissionRequest.hooks]]
type = "command"
command = "sidekick-agent-status ready"

[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "sidekick-agent-status done"
```

## Development

Run the app from source:

```bash
cargo run --bin sidekick
```

Run a quick compile check:

```bash
cargo check
```

Build all binaries:

```bash
cargo build --bins
```

The main modules are:

- `src/main.rs`: GTK application setup, layout, shortcuts, IPC dispatch.
- `src/tab.rs`: VTE terminal construction and tab titles.
- `src/pane.rs`: terminal splitting, closing, and focus navigation.
- `src/filetree.rs`: cwd-aware project tree.
- `src/editor.rs`: editor tabs backed by GtkSourceView.
- `src/git.rs` and `src/gitpanel.rs`: git status and changed-file UI.
- `src/searchpanel.rs`: file content search (ripgrep/grep).
- `src/runpanel.rs`: task runner with global and project-local tasks.
- `src/agentpanel.rs`: agents dashboard panel.
- `src/hostspanel.rs`: ssh / teleport hosts panel.
- `src/palette.rs`: command palette.
- `src/shortcutshelp.rs`: keyboard shortcuts help window.
- `src/diff.rs`: colored diff views and accept/reject diff tabs.
- `src/browser.rs`: optional WebKit side panel.
- `src/config.rs`: configuration loading and defaults.
- `src/ipc.rs`: Unix-socket IPC server.

## Contributing

Issues and pull requests are welcome. Good first improvements include distro
dependency notes, packaging, additional shell integrations, editor polish, and
expanded theme support.

Before opening a pull request, please run:

```bash
cargo fmt
cargo check
```

## License

MIT. See [LICENSE](LICENSE).
