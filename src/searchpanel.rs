use gtk4::prelude::*;

const MAX_MATCHES_PER_FILE: usize = 5;
const MAX_FILES: usize = 50;
const MAX_SEARCH_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

/// Separator between line number and path in row widget names. Newlines
/// cannot appear in paths, unlike ':'.
pub const ROW_NAME_SEP: char = '\n';

pub fn row_name(line: u32, abs_path: &str) -> String {
    format!("{}{}{}", line, ROW_NAME_SEP, abs_path)
}

#[derive(Clone)]
pub struct FileMatches {
    pub abs_path: String,
    pub rel_path: String,
    pub lines: Vec<(u32, String)>,
    pub capped: bool, // true if there may be more matches than shown
}

pub fn build() -> (gtk4::Box, gtk4::Entry, gtk4::ListBox) {
    let header = gtk4::Label::new(Some("SEARCH"));
    header.set_xalign(0.0);
    header.add_css_class("sidebar-header");

    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("Search files…"));
    entry.set_margin_start(8);
    entry.set_margin_end(8);
    entry.set_margin_top(4);
    entry.set_margin_bottom(4);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    list.add_css_class("file-tree");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);

    let panel = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    panel.append(&header);
    panel.append(&entry);
    panel.append(&scroll);

    (panel, entry, list)
}

pub fn populate(list: &gtk4::ListBox, files: &[FileMatches]) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }

    if files.is_empty() {
        show_message(list, "No results");
        return;
    }

    for file in files {
        // File header row — clicking opens the file at the first match
        let header_row = gtk4::ListBoxRow::new();
        let first_line = file.lines.first().map(|(n, _)| *n).unwrap_or(0);
        header_row.set_widget_name(&row_name(first_line, &file.abs_path));

        let count_str = if file.capped {
            format!("{}+", file.lines.len())
        } else {
            file.lines.len().to_string()
        };
        let label = gtk4::Label::new(Some(&format!("{}  ({})", file.rel_path, count_str)));
        label.set_xalign(0.0);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_margin_top(5);
        label.set_margin_bottom(2);
        label.set_ellipsize(pango::EllipsizeMode::Start);
        label.add_css_class("search-result-file");
        header_row.set_child(Some(&label));
        list.insert(&header_row, -1);

        // Individual match rows
        for (line_num, text) in &file.lines {
            let match_row = gtk4::ListBoxRow::new();
            match_row.set_widget_name(&row_name(*line_num, &file.abs_path));

            let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
            hbox.set_margin_start(20);
            hbox.set_margin_end(8);
            hbox.set_margin_top(1);
            hbox.set_margin_bottom(1);

            let line_label = gtk4::Label::new(Some(&line_num.to_string()));
            line_label.set_width_chars(4);
            line_label.set_xalign(1.0);
            line_label.add_css_class("search-result-line");

            let text_label = gtk4::Label::new(Some(text.trim()));
            text_label.set_xalign(0.0);
            text_label.set_hexpand(true);
            text_label.set_ellipsize(pango::EllipsizeMode::End);
            text_label.add_css_class("search-result-text");

            hbox.append(&line_label);
            hbox.append(&text_label);
            match_row.set_child(Some(&hbox));
            list.insert(&match_row, -1);
        }
    }
}

/// Clear the list and show a single non-interactive message row.
pub fn show_message(list: &gtk4::ListBox, text: &str) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    let row = gtk4::ListBoxRow::new();
    let label = gtk4::Label::new(Some(text));
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    label.set_margin_start(8);
    label.set_margin_end(8);
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.add_css_class("sidebar-header");
    row.set_child(Some(&label));
    row.set_activatable(false);
    row.set_selectable(false);
    list.insert(&row, -1);
}

pub fn run_search(root: &str, query: &str) -> Vec<FileMatches> {
    if query.trim().is_empty() {
        return vec![];
    }

    let max = MAX_MATCHES_PER_FILE.to_string();
    // --fixed-strings: the sidebar searches literally; a query like "foo("
    // must not be treated as a (broken) regex. Output is capped and truncated
    // so a huge repo cannot balloon memory.
    let rg = crate::limits::command_stdout_limited(
        std::process::Command::new("rg")
            .args([
                "--line-number",
                "--color=never",
                "--fixed-strings",
                "--max-count",
                &max,
                "--",
                query,
            ])
            .current_dir(root),
        MAX_SEARCH_OUTPUT_BYTES,
        &[1], // 1 = no matches
        crate::limits::CapMode::Truncate,
    );

    let raw = match rg {
        Ok(out) => out,
        Err(_) => {
            // rg not available — fall back to grep, skipping binary files and
            // the usual junk directories rg's gitignore handling would skip.
            match crate::limits::command_stdout_limited(
                std::process::Command::new("grep")
                    .args([
                        "-rnIF",
                        "--color=never",
                        "--exclude-dir=.git",
                        "--exclude-dir=node_modules",
                        "--exclude-dir=target",
                        "-m",
                        &max,
                        "--",
                        query,
                        ".",
                    ])
                    .current_dir(root),
                MAX_SEARCH_OUTPUT_BYTES,
                &[1],
                crate::limits::CapMode::Truncate,
            ) {
                Ok(out) => out,
                Err(_) => return vec![],
            }
        }
    };

    let text = String::from_utf8_lossy(&raw);
    let mut map: indexmap::IndexMap<String, FileMatches> = indexmap::IndexMap::new();

    for line in text.lines() {
        let mut parts = line.splitn(3, ':');
        let rel = match parts.next() {
            Some(r) => r.trim_start_matches("./").to_string(),
            None => continue,
        };
        let line_num: u32 = match parts.next().and_then(|n| n.parse().ok()) {
            Some(n) => n,
            None => continue,
        };
        let content = parts.next().unwrap_or("").to_string();

        // Stop only when a *new* file would exceed the cap, so the last
        // included file still gets all of its matches.
        if map.len() >= MAX_FILES && !map.contains_key(&rel) {
            break;
        }

        let abs = format!("{}/{}", root, rel);
        let entry = map.entry(rel.clone()).or_insert_with(|| FileMatches {
            abs_path: abs,
            rel_path: rel,
            lines: vec![],
            capped: false,
        });
        entry.lines.push((line_num, content));
        if entry.lines.len() == MAX_MATCHES_PER_FILE {
            entry.capped = true;
        }
    }

    map.into_values().collect()
}
