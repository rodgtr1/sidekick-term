#![allow(deprecated)]

use glib::prelude::{ObjectExt, ToValue};
use gtk4::prelude::{TreeModelExt, TreeModelExtManual, TreeViewExt, WidgetExt};
use pango::EllipsizeMode;

pub const COL_NAME: u32 = 0;
pub const COL_PATH: u32 = 1;
pub const COL_IS_DIR: u32 = 2;

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

pub fn populate(store: &gtk4::TreeStore, root: &str) {
    store.clear();
    populate_dir(store, None, root, 0);
}

pub fn populate_subtree(store: &gtk4::TreeStore, parent: &gtk4::TreeIter, path: &str) {
    populate_dir(store, Some(parent), path, 1);
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

pub fn has_children(store: &gtk4::TreeStore, iter: &gtk4::TreeIter) -> bool {
    store.iter_children(Some(iter)).is_some()
}

fn populate_dir(store: &gtk4::TreeStore, parent: Option<&gtk4::TreeIter>, path: &str, depth: u32) {
    let mut entries: Vec<(String, String, bool)> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let full = e.path().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            Some((name, full, is_dir))
        })
        .collect();

    entries.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));

    for (name, full, is_dir) in entries {
        let iter = store.append(parent);
        store.set_value(&iter, COL_NAME, &name.to_value());
        store.set_value(&iter, COL_PATH, &full.to_value());
        store.set_value(&iter, COL_IS_DIR, &is_dir.to_value());
        if is_dir && depth == 0 {
            populate_dir(store, Some(&iter), &full, depth + 1);
        }
    }
}
