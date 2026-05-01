#![allow(deprecated)]

use glib::prelude::{ObjectExt, ToValue};
use gtk4::prelude::{TreeModelExt, TreeModelExtManual, TreeViewExt, WidgetExt};
use pango::EllipsizeMode;

pub const COL_NAME: u32 = 0;
pub const COL_PATH: u32 = 1;
pub const COL_IS_DIR: u32 = 2;
pub const MAX_DEPTH: u32 = 1;

#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
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
    let store = gtk4::TreeStore::new(&[glib::Type::STRING, glib::Type::STRING, glib::Type::BOOL]);

    let tree = gtk4::TreeView::new();
    tree.set_model(Some(&store));
    tree.set_headers_visible(false);
    tree.set_enable_tree_lines(false);
    tree.set_show_expanders(true);
    tree.add_css_class("file-tree");

    let col = gtk4::TreeViewColumn::new();
    let icon_cell = gtk4::CellRendererPixbuf::new();
    let text_cell = gtk4::CellRendererText::new();

    // Eliminate cell renderer padding so rows are compact
    icon_cell.set_property("ypad", 1u32);
    icon_cell.set_property("xpad", 2u32);
    text_cell.set_property("ypad", 1u32);

    col.set_sizing(gtk4::TreeViewColumnSizing::Fixed);
    col.set_fixed_width(230);

    text_cell.set_property("ellipsize", EllipsizeMode::End);

    col.pack_start(&icon_cell, false);
    col.pack_start(&text_cell, true);
    col.add_attribute(&text_cell, "text", COL_NAME as i32);

    col.set_cell_data_func(
        &icon_cell,
        |_col: &gtk4::TreeViewColumn,
         cell: &gtk4::CellRenderer,
         model: &gtk4::TreeModel,
         iter: &gtk4::TreeIter| {
            let is_dir = model
                .get_value(iter, COL_IS_DIR as i32)
                .get::<bool>()
                .unwrap_or(false);
            cell.set_property(
                "icon-name",
                if is_dir {
                    "folder-symbolic"
                } else {
                    "text-x-generic-symbolic"
                },
            );
        },
    );

    tree.append_column(&col);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&tree));
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);

    // Header label showing current folder name
    let header = gtk4::Label::new(Some("~"));
    header.set_xalign(0.0);
    header.add_css_class("sidebar-header");

    (header, tree, store, scroll)
}

pub fn scan_root(root: &str) -> Vec<TreeEntry> {
    scan_dir(root, 0)
}

pub fn scan_subtree(path: &str) -> Vec<TreeEntry> {
    scan_dir(path, 1)
}

pub fn apply_root(store: &gtk4::TreeStore, entries: &[TreeEntry]) {
    store.clear();
    apply_entries(store, None, entries);
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
    let iter = store.iter_first()?;
    find_iter_recursive(store, &iter, file_path)
}

pub fn has_children(store: &gtk4::TreeStore, iter: &gtk4::TreeIter) -> bool {
    store.iter_children(Some(iter)).is_some()
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

fn scan_dir(path: &str, depth: u32) -> Vec<TreeEntry> {
    let mut entries: Vec<TreeEntry> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .take(crate::limits::MAX_DIRECTORY_ENTRIES)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let full = e.path().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            Some(TreeEntry {
                name,
                path: full,
                is_dir,
                children: Vec::new(),
            })
        })
        .collect();

    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));

    if depth < MAX_DEPTH {
        for entry in &mut entries {
            if entry.is_dir {
                entry.children = scan_dir(&entry.path, depth + 1);
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
        apply_entries(store, Some(&iter), &entry.children);
    }
}
