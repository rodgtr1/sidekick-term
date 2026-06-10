# Mac Parity Features — Design

Date: 2026-06-10
Status: Approved scope (user requested porting Mac-only features in a native Linux way)

## Background

The macOS sidekick (Swift/AppKit) has several features the Linux GTK4 version
lacks. Comparison identified these Mac-only features, in impact order:

1. Agent dashboard sidebar panel (every tab's agent state + elapsed time, click to jump)
2. Richer shell integration (per-command exit code/duration, long-command notifications)
3. Hosts panel (~/.ssh/config + Teleport `tsh ls`, click to connect in a new tab)
4. Command palette
5. Keyboard shortcuts help panel
6. GUI preferences window
7. Per-hunk diff accept/reject

## Scope

In scope: items 1–5, implemented with native GTK4 idioms and the existing
VTE termprop plumbing.

Out of scope, with rationale:

- **GUI preferences window** — on Linux, `Ctrl+,` already opens `config.toml`
  in nvim, which fits this app's terminal-first audience. YAGNI.
- **Per-hunk accept/reject** — the Claude Code `PreToolUse` hook protocol is a
  binary allow/block decision, so per-hunk acceptance cannot feed back a
  partially-applied edit. The Mac per-hunk UI is cosmetic in that flow. Skip.

## Design

### 1. Agent dashboard panel (`src/agentpanel.rs`)

A fifth activity-bar panel ("agents") in the existing `gtk4::Stack`.

- A `ListBox` where each row shows: tab title, agent state label
  (RUN/WAIT/DONE/IDLE) colored like the existing tab dots, and elapsed time in
  that state.
- Data source: poll once per second (same pattern as the existing activity-bar
  badge). Iterate notebook pages, `pane::collect_terminals_pub(&page)`, look up
  each terminal pointer in the `AgentMap` to get `(tab_id, AgentCell)`. Dedupe
  by tab id (split panes share one cell).
- Elapsed time: the panel keeps its own `HashMap<u64, (AgentState, Instant)>`;
  when a polled state differs from the cached one, reset the timestamp. This
  requires zero changes to the existing agent-state plumbing.
- Row activation: `notebook.set_current_page(page_index)` and focus the first
  terminal.
- Keyboard shortcut: `Ctrl+Shift+A`. Activity bar gets a fifth button.

### 2. Command exit/duration via termprops (`shell-integration.zsh` + `main.rs`)

The Mac app parses OSC 133 text. The native Linux equivalent is VTE termprops,
which the app already uses for precmd/preexec and agent status.

- Install one more custom termprop: `vte.ext.sidekick.exit` (string).
- `shell-integration.zsh` `_sidekick_precmd` additionally emits
  `\033]666;vte.ext.sidekick.exit=<$?>\033\\` before the precmd signal.
- Duration is computed in-app: `wire_agent_state_handlers` records an
  `Instant` on `vte.shell.preexec` and reads it on `vte.shell.precmd`.
- Behavior: when a command that ran ≥ 15 seconds finishes in an unfocused
  window or a background tab, send a desktop notification ("Command finished
  (exit 0) in 1m 23s — <tab title>") using the same `gio::Notification`
  pattern as `notify_agent_attention`. No persistent UI; the existing red tab
  dot already covers in-app signaling.

### 3. Hosts panel (`src/hostspanel.rs`)

A sixth activity-bar panel listing connectable hosts.

- Sources: `Host` entries from `~/.ssh/config` (skipping wildcard patterns and
  negations), and Teleport nodes from `tsh ls -f json` when `tsh` is on PATH
  and logged in. Teleport listing runs in a worker thread with results sent
  back over an `async_channel` (same pattern as quickopen search).
- Each row: host name + section header (SSH / Teleport). Activating a row
  opens a new tab running `ssh <host>` or `tsh ssh <host>` via the existing
  new-tab-with-startup-command path.
- Refresh button in the panel header. No keyboard shortcut (activity-bar
  click only) to avoid burning another Ctrl+Shift chord.
- SSH config parsing is pure Rust with unit tests.

### 4. Command palette (`src/palette.rs`)

Modeled on `quickopen.rs`: modal undecorated window, `Entry` + `ListBox`,
substring/fuzzy filter, Enter/click to run, Escape to close.

- `PaletteAction { title: &'static str, shortcut: Option<&'static str>, run: Rc<dyn Fn()> }`.
- The action list is built in `main.rs` where all the closures (new tab,
  splits, panel switching, browser, zoom, config, shortcuts help) already
  live, and passed to `palette::show`.
- Filtering is case-insensitive subsequence matching, unit tested.
- Keyboard shortcut: `Ctrl+Shift+P`.

### 5. Keyboard shortcuts help (`src/shortcutshelp.rs`)

A modal window with a scrollable two-column grid (shortcut, action) compiled
from the README table, grouped by section. GtkShortcutsWindow is deprecated in
recent GTK, so this is a plain `gtk4::Window` + `Grid`.

- Keyboard shortcut: `Ctrl+Shift+?` (Ctrl+Shift+slash), plus a palette entry.

## New keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+A` | Agents dashboard panel |
| `Ctrl+Shift+P` | Command palette |
| `Ctrl+Shift+?` | Keyboard shortcuts help |

None conflict with existing bindings (C, V, T, W, Tab, D, X, H, E, G, F, R, B, O, ,).

## Testing

- Unit tests for: ssh config parsing, palette filtering, duration formatting.
- `cargo check` + `cargo fmt` after each feature; manual smoke test via
  `cargo run` is left to the user (GUI).

## Error handling

- Missing `~/.ssh/config` or `tsh`: hosts panel shows section messages, never
  errors.
- Shell integration not installed: no exit termprop arrives; command
  notifications simply never fire (same degradation as the existing dot).
