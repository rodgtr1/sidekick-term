#![allow(deprecated)]

use glib::prelude::{ObjectExt, ToValue};
use gtk4::prelude::{TreeModelExt, TreeModelExtManual, TreeViewExt, WidgetExt};
use pango::EllipsizeMode;

pub const COL_NAME: u32 = 0;
pub const COL_PATH: u32 = 1;
pub const COL_IS_DIR: u32 = 2;
pub const COL_IGNORED: u32 = 3;
const MAX_DEPTH: u32 = 2;
const MAX_TREE_TOTAL_ENTRIES: usize = 5000;
pub const PLACEHOLDER_PATH: &str = "//placeholder//";
pub const LOADING_PATH: &str = "//loading//";

#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub ignored: bool,
    pub children: Vec<TreeEntry>,
}

/// Returns (header label, tree view, tree store, scrolled window).
/// The caller is responsible for assembling these into the sidebar.
pub fn build() -> (
    gtk4::Label,
    gtk4::TreeView,
    gtk4::TreeStore,
    gtk4::ScrolledWindow,
) {
    let store = gtk4::TreeStore::new(&[
        glib::Type::STRING,
        glib::Type::STRING,
        glib::Type::BOOL,
        glib::Type::BOOL,
    ]);

    let tree = gtk4::TreeView::new();
    tree.set_model(Some(&store));
    tree.set_headers_visible(false);
    tree.set_enable_tree_lines(false);
    tree.set_show_expanders(true);
    tree.add_css_class("file-tree");

    let col = gtk4::TreeViewColumn::new();
    let icon_cell = gtk4::CellRendererPixbuf::new();
    let text_cell = gtk4::CellRendererText::new();

    icon_cell.set_property("ypad", 1u32);
    icon_cell.set_property("xpad", 2u32);
    text_cell.set_property("ypad", 1u32);

    col.set_sizing(gtk4::TreeViewColumnSizing::Fixed);
    col.set_fixed_width(230);

    text_cell.set_property("ellipsize", EllipsizeMode::End);

    col.pack_start(&icon_cell, false);
    col.pack_start(&text_cell, true);

    col.set_cell_data_func(
        &icon_cell,
        |_col: &gtk4::TreeViewColumn,
         cell: &gtk4::CellRenderer,
         model: &gtk4::TreeModel,
         iter: &gtk4::TreeIter| {
            let path = model
                .get_value(iter, COL_PATH as i32)
                .get::<String>()
                .unwrap_or_default();
            if path == PLACEHOLDER_PATH || path == LOADING_PATH {
                cell.set_property("gicon", &gio::ThemedIcon::new("text-x-generic"));
                return;
            }
            let is_dir = model
                .get_value(iter, COL_IS_DIR as i32)
                .get::<bool>()
                .unwrap_or(false);
            let name = model
                .get_value(iter, COL_NAME as i32)
                .get::<String>()
                .unwrap_or_default();
            let content_type = if is_dir {
                "inode/directory".to_string()
            } else {
                gio::content_type_guess(Some(name.as_str()), None::<&[u8]>)
                    .0
                    .to_string()
            };
            let icon = gio::content_type_get_icon(&content_type);
            cell.set_property("icon-name", "");
            cell.set_property("gicon", &icon);
        },
    );

    col.set_cell_data_func(
        &text_cell,
        |_col: &gtk4::TreeViewColumn,
         cell: &gtk4::CellRenderer,
         model: &gtk4::TreeModel,
         iter: &gtk4::TreeIter| {
            let path = model
                .get_value(iter, COL_PATH as i32)
                .get::<String>()
                .unwrap_or_default();
            if path == PLACEHOLDER_PATH {
                cell.set_property("text", "");
                cell.set_property("foreground", "#45475a");
                return;
            }
            if path == LOADING_PATH {
                cell.set_property("text", "Loading…");
                cell.set_property("foreground", "#45475a");
                return;
            }
            let name = model
                .get_value(iter, COL_NAME as i32)
                .get::<String>()
                .unwrap_or_default();
            let ignored = model
                .get_value(iter, COL_IGNORED as i32)
                .get::<bool>()
                .unwrap_or(false);
            cell.set_property("text", name);
            cell.set_property("foreground", if ignored { "#45475a" } else { "#cdd6f4" });
        },
    );

    tree.append_column(&col);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&tree));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);

    let header = gtk4::Label::new(Some("~"));
    header.set_xalign(0.0);
    header.add_css_class("sidebar-header");

    (header, tree, store, scroll)
}

pub fn scan_root(root: &str) -> Vec<TreeEntry> {
    let ignored = crate::git::ignored_set(root);
    let mut budget = MAX_TREE_TOTAL_ENTRIES;
    scan_dir_budgeted(root, 0, &ignored, &mut budget)
}

pub fn scan_subtree(path: &str) -> Vec<TreeEntry> {
    let root = crate::git::repo_root(path).unwrap_or_else(|| path.to_string());
    let ignored = crate::git::ignored_set(&root);
    let mut budget = MAX_TREE_TOTAL_ENTRIES;
    scan_dir_budgeted(path, 0, &ignored, &mut budget)
}

/// Bulk-load entries into the store. Detaches the view during population so
/// GTK reads the complete model state on reconnect instead of processing
/// O(n²) incremental signals.
pub fn apply_root(store: &gtk4::TreeStore, tree_view: &gtk4::TreeView, entries: &[TreeEntry]) {
    tree_view.set_model(None::<&gtk4::TreeStore>);
    store.clear();
    apply_entries(store, None, entries);
    tree_view.set_model(Some(store));
}

pub fn apply_subtree(store: &gtk4::TreeStore, parent: &gtk4::TreeIter, entries: &[TreeEntry]) {
    clear_children(store, parent);
    apply_entries(store, Some(parent), entries);
}

pub fn row_info(store: &gtk4::TreeStore, iter: &gtk4::TreeIter) -> (String, bool) {
    let path = store
        .get_value(iter, COL_PATH as i32)
        .get::<String>()
        .unwrap_or_default();
    let is_dir = store
        .get_value(iter, COL_IS_DIR as i32)
        .get::<bool>()
        .unwrap_or(false);
    (path, is_dir)
}

pub fn iter_for_path(store: &gtk4::TreeStore, path: &gtk4::TreePath) -> Option<gtk4::TreeIter> {
    store.iter(path)
}

pub fn find_iter_by_file_path(store: &gtk4::TreeStore, file_path: &str) -> Option<gtk4::TreeIter> {
    let mut iter = store.iter_first()?;
    loop {
        if let Some(found) = find_iter_recursive(store, &iter, file_path) {
            return Some(found);
        }
        if !store.iter_next(&mut iter) {
            return None;
        }
    }
}

pub fn has_placeholder(store: &gtk4::TreeStore, iter: &gtk4::TreeIter) -> bool {
    match store.iter_children(Some(iter)) {
        None => false,
        Some(child) => store
            .get_value(&child, COL_PATH as i32)
            .get::<String>()
            .map(|p| p == PLACEHOLDER_PATH)
            .unwrap_or(false),
    }
}

/// Replace the placeholder child with a "Loading…" row so the parent row
/// stays visually open while the subtree scan runs in the background.
pub fn set_loading(store: &gtk4::TreeStore, parent: &gtk4::TreeIter) {
    if let Some(child) = store.iter_children(Some(parent)) {
        store.remove(&child);
    }
    let row = store.append(Some(parent));
    store.set_value(&row, COL_NAME, &"Loading…".to_value());
    store.set_value(&row, COL_PATH, &LOADING_PATH.to_value());
    store.set_value(&row, COL_IS_DIR, &false.to_value());
    store.set_value(&row, COL_IGNORED, &false.to_value());
}

fn find_iter_recursive(
    store: &gtk4::TreeStore,
    iter: &gtk4::TreeIter,
    file_path: &str,
) -> Option<gtk4::TreeIter> {
    let current = store
        .get_value(iter, COL_PATH as i32)
        .get::<String>()
        .unwrap_or_default();
    if current == file_path {
        return Some(*iter);
    }
    if let Some(mut child) = store.iter_children(Some(iter)) {
        loop {
            if let Some(found) = find_iter_recursive(store, &child, file_path) {
                return Some(found);
            }
            if !store.iter_next(&mut child) {
                break;
            }
        }
    }
    None
}

fn clear_children(store: &gtk4::TreeStore, parent: &gtk4::TreeIter) {
    while let Some(child) = store.iter_children(Some(parent)) {
        store.remove(&child);
    }
}

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
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".git" {
                return None;
            }
            let full = e.path().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let entry_ignored = ignored.contains(&full);
            Some(TreeEntry {
                name,
                path: full,
                is_dir,
                ignored: entry_ignored,
                children: Vec::new(),
            })
        })
        .collect();

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
            // At max depth: peek to see if this dir has any content so we can
            // show an expand arrow. One read_dir call, no recursion.
            let has_content = std::fs::read_dir(&entry.path)
                .ok()
                .and_then(|mut d| d.next())
                .is_some();
            if has_content {
                entry.children = vec![TreeEntry {
                    name: String::new(),
                    path: PLACEHOLDER_PATH.to_string(),
                    is_dir: false,
                    ignored: false,
                    children: vec![],
                }];
            }
        }
    }

    entries
}

fn apply_entries(store: &gtk4::TreeStore, parent: Option<&gtk4::TreeIter>, entries: &[TreeEntry]) {
    for entry in entries {
        let iter = store.append(parent);
        store.set_value(&iter, COL_NAME, &entry.name.to_value());
        store.set_value(&iter, COL_PATH, &entry.path.to_value());
        store.set_value(&iter, COL_IS_DIR, &entry.is_dir.to_value());
        store.set_value(&iter, COL_IGNORED, &entry.ignored.to_value());
        apply_entries(store, Some(&iter), &entry.children);
    }
}

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
