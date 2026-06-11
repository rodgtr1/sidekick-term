use gtk4::prelude::*;
use similar::{ChangeTag, TextDiff};

pub fn open_message(title: &str, path: &str, message: &str, notebook: &gtk4::Notebook) {
    let buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);
    buffer.set_text(&format!("{path}\n\n{message}"));

    let view = gtk4::TextView::with_buffer(&buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.add_css_class("editor-view");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&view));
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);

    let tab_label = gtk4::Label::new(Some(title));
    let page_idx = notebook.n_pages();
    notebook.append_page(&scroll, Some(&tab_label));
    notebook.set_tab_reorderable(&scroll, true);
    notebook.set_current_page(Some(page_idx));
    view.grab_focus();
}

pub fn open(
    path: &str,
    old: &str,
    new_content: &str,
    notebook: &gtk4::Notebook,
    decision: async_channel::Sender<bool>,
) {
    if old.len() > crate::limits::MAX_DIFF_BYTES
        || new_content.len() > crate::limits::MAX_DIFF_BYTES
    {
        open_message(
            "diff too large",
            path,
            "Diff content is too large to preview safely.",
            notebook,
        );
        let _ = decision.send_blocking(false);
        return;
    }

    let filename = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    let buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);

    // Color tags
    let tag_add = buffer.create_tag(Some("add"), &[]).unwrap();
    tag_add.set_property("foreground", "#a6e3a1");
    tag_add.set_property("background", "#1c3829");

    let tag_del = buffer.create_tag(Some("del"), &[]).unwrap();
    tag_del.set_property("foreground", "#f38ba8");
    tag_del.set_property("background", "#3d1422");

    let tag_hunk = buffer.create_tag(Some("hunk"), &[]).unwrap();
    tag_hunk.set_property("foreground", "#89b4fa");

    let tag_ctx = buffer.create_tag(Some("ctx"), &[]).unwrap();
    tag_ctx.set_property("foreground", "#6c7086");

    let diff = TextDiff::from_lines(old, new_content);
    let mut text = String::new();
    let mut spans: Vec<(usize, usize, &'static str)> = Vec::new(); // (byte_start, byte_end, tag)

    for group in diff.grouped_ops(3) {
        // Hunk header
        let first = group.first().unwrap();
        let last = group.last().unwrap();
        let old_start = first.old_range().start + 1;
        let new_start = first.new_range().start + 1;
        let old_len = last.old_range().end - first.old_range().start;
        let new_len = last.new_range().end - first.new_range().start;
        let hunk_line = format!(
            "@@ -{},{} +{},{} @@\n",
            old_start, old_len, new_start, new_len
        );
        let start = text.len();
        text.push_str(&hunk_line);
        spans.push((start, text.len(), "hunk"));

        for op in &group {
            for change in diff.iter_changes(op) {
                let prefix = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                let line = format!("{}{}", prefix, change.value());
                let start = text.len();
                text.push_str(&line);
                if !line.ends_with('\n') {
                    text.push('\n');
                }
                let tag = match change.tag() {
                    ChangeTag::Delete => Some("del"),
                    ChangeTag::Insert => Some("add"),
                    ChangeTag::Equal => Some("ctx"),
                };
                if let Some(t) = tag {
                    spans.push((start, text.len(), t));
                }
            }
        }
    }

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

    let view = gtk4::TextView::with_buffer(&buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.add_css_class("editor-view");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_child(Some(&view));
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);

    // Accept / Reject buttons
    let accept_btn = gtk4::Button::with_label("Accept");
    accept_btn.add_css_class("suggested-action");
    let reject_btn = gtk4::Button::with_label("Reject");
    reject_btn.add_css_class("destructive-action");

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_margin_top(8);
    btn_box.set_margin_bottom(8);
    btn_box.set_margin_start(12);
    btn_box.set_margin_end(12);
    btn_box.set_halign(gtk4::Align::End);
    btn_box.append(&reject_btn);
    btn_box.append(&accept_btn);

    let path_label = gtk4::Label::new(Some(path));
    path_label.set_xalign(0.0);
    path_label.set_margin_start(12);
    path_label.set_margin_top(6);
    path_label.set_margin_bottom(6);
    path_label.set_ellipsize(pango::EllipsizeMode::Start);
    path_label.set_hexpand(true);

    let top_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    top_bar.append(&path_label);
    top_bar.append(&btn_box);

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    vbox.append(&top_bar);
    vbox.append(&scroll);

    let tab_label = gtk4::Label::new(Some(&format!("diff: {}", filename)));

    let decided = std::rc::Rc::new(std::cell::Cell::new(false));

    {
        let tx = decision.clone();
        let nb = notebook.clone();
        let vbox_c = vbox.clone();
        let decided = decided.clone();
        accept_btn.connect_clicked(move |_| {
            if decided.get() {
                return;
            }
            decided.set(true);
            let _ = tx.send_blocking(true);
            if let Some(n) = nb.page_num(&vbox_c) {
                nb.remove_page(Some(n));
            }
        });
    }
    {
        let tx = decision.clone();
        let nb = notebook.clone();
        let vbox_c = vbox.clone();
        let decided = decided.clone();
        reject_btn.connect_clicked(move |_| {
            if decided.get() {
                return;
            }
            decided.set(true);
            let _ = tx.send_blocking(false);
            if let Some(n) = nb.page_num(&vbox_c) {
                nb.remove_page(Some(n));
            }
        });
    }

    // If the user closes the tab without clicking, treat as reject
    {
        let tx = decision.clone();
        let decided = decided.clone();
        let vbox_w = vbox.downgrade();
        notebook.connect_page_removed(move |_, widget, _| {
            if let Some(v) = vbox_w.upgrade() {
                if widget == &v && !decided.get() {
                    decided.set(true);
                    let _ = tx.send_blocking(false);
                }
            }
        });
    }

    let page_idx = notebook.n_pages();
    notebook.append_page(&vbox, Some(&tab_label));
    notebook.set_tab_reorderable(&vbox, true);
    notebook.set_current_page(Some(page_idx));
    view.grab_focus();
}

pub fn open_readonly(title: &str, diff_text: &str, notebook: &gtk4::Notebook) {
    if diff_text.trim().is_empty() {
        return;
    }
    if diff_text.len() > crate::limits::MAX_DIFF_BYTES {
        open_message(
            "diff too large",
            title,
            "Diff content is too large to preview safely.",
            notebook,
        );
        return;
    }

    let buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);

    let tag_add = buffer.create_tag(Some("add"), &[]).unwrap();
    let tag_del = buffer.create_tag(Some("del"), &[]).unwrap();
    let tag_hunk = buffer.create_tag(Some("hunk"), &[]).unwrap();
    let tag_ctx = buffer.create_tag(Some("ctx"), &[]).unwrap();
    tag_add.set_property("foreground", "#a6e3a1");
    tag_add.set_property("background", "#1c3829");
    tag_del.set_property("foreground", "#f38ba8");
    tag_del.set_property("background", "#3d1422");
    tag_hunk.set_property("foreground", "#89b4fa");
    tag_ctx.set_property("foreground", "#6c7086");

    let mut text = String::new();
    let mut spans: Vec<(usize, usize, &'static str)> = Vec::new();

    for line in diff_text.lines() {
        let start = text.len();
        text.push_str(line);
        text.push('\n');
        let tag = if line.starts_with("@@") {
            "hunk"
        } else if line.starts_with("+++ ")
            || line.starts_with("--- ")
            || line.starts_with("diff ")
            || line.starts_with("index ")
        {
            "ctx"
        } else if line.starts_with('+') {
            "add"
        } else if line.starts_with('-') {
            "del"
        } else {
            "ctx"
        };
        spans.push((start, text.len(), tag));
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

    let tab_label = gtk4::Label::new(Some(&format!("Δ {}", title)));
    let page_idx = notebook.n_pages();
    notebook.append_page(&scroll, Some(&tab_label));
    notebook.set_tab_reorderable(&scroll, true);
    notebook.set_current_page(Some(page_idx));
    view.grab_focus();
}

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

#[cfg(test)]
mod tests {
    use super::char_offsets;

    fn naive(text: &str, b: usize) -> usize {
        text[..b.min(text.len())].chars().count()
    }

    #[test]
    fn char_offsets_matches_naive_with_multibyte() {
        // Byte offsets must fall on char boundaries (as real diff spans do).
        // "héllo\n" = h é(2 bytes) l l o \n  -> boundaries at 0,1,3,4,5,6,7
        let text = "héllo\nwörld\n€nd\n";
        let bytes = vec![0, 1, 3, 6, 7, text.len()];
        let got = char_offsets(text, &bytes);
        let want: Vec<usize> = bytes.iter().map(|&b| naive(text, b)).collect();
        assert_eq!(got, want);
    }
}
