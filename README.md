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
- Git changes panel for the active repository, split into staged and unstaged
  files.
- Read-only colored diff tabs for changed files.
- Optional embedded browser panel for quick docs/searches.
- Configurable terminal font, cursor, padding, scrollback, hyperlink behavior,
  bell, and opacity.
- Shell integration for background tab notification dots.
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

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+T` | New terminal tab |
| `Ctrl+Shift+W` | Close the current pane, editor tab, or diff tab |
| `Ctrl+Tab` | Next tab |
| `Ctrl+Shift+Tab` | Previous tab |
| `Ctrl+Shift+D` | Split terminal right |
| `Ctrl+Shift+X` | Split terminal down |
| `Alt+Left` | Focus previous terminal pane |
| `Alt+Right` | Focus next terminal pane |
| `Ctrl+Shift+E` | Show file explorer panel |
| `Ctrl+Shift+G` | Show git panel |
| `Ctrl+F` | Quick open: search file names |
| `Ctrl+Shift+F` | Show search-in-files panel |
| `Ctrl+Shift+R` | Show run panel |
| `Ctrl+Shift+B` | Toggle sidebar |
| `Ctrl+Shift+O` | Toggle embedded browser panel |
| `Ctrl+S` | Save the current editor tab |

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
```

## Shell Integration

Shell integration enables the red notification dot on background tabs when a
command finishes and the shell returns to the prompt.

For zsh:

```bash
mkdir -p ~/.config/sidekick
cp shell-integration.zsh ~/.config/sidekick/
echo 'source ~/.config/sidekick/shell-integration.zsh' >> ~/.zshrc
```

Restart your shell or source `~/.zshrc`. Without this integration, sidekick still
works, but background tabs will not show command-completion dots.

## Command-Line Control

`sidekick` listens on:

```text
~/.local/run/sidekick.sock
```

You can check that a running instance is reachable:

```bash
sidekick-ctl ping
```

Open a new tab in the running instance:

```bash
sidekick-ctl new-tab
```

## Claude Code Hook

`sidekick-hook` is optional. It is designed for Claude Code's `PreToolUse` hook and
can show proposed `Write`, `Edit`, and `MultiEdit` changes as diffs inside sidekick.
Accepting the diff lets the edit proceed; rejecting exits with code `2`.

Build or install sidekick, then place the hook where Claude Code expects it:

```bash
mkdir -p ~/.claude/hooks/PreToolUse
cp ~/.local/bin/sidekick-hook ~/.claude/hooks/PreToolUse/sidekick-hook
chmod +x ~/.claude/hooks/PreToolUse/sidekick-hook
```

If sidekick is not running, the hook allows edits to proceed normally.

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
