use crate::{config, tab};
use gtk4::prelude::*;
use vte4::prelude::*;

pub fn split(
    window: &gtk4::ApplicationWindow,
    notebook: &gtk4::Notebook,
    cfg: &config::Config,
    orientation: gtk4::Orientation,
) {
    let focused = match focused_terminal(window) {
        Some(t) => t,
        None => return,
    };
    let focused_w: gtk4::Widget = focused.clone().upcast();

    let new_term = tab::build(cfg);
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    new_term.spawn_async(
        vte4::PtyFlags::DEFAULT,
        None,
        &[shell.as_str()],
        &[],
        glib::SpawnFlags::DEFAULT,
        || {},
        -1,
        None::<&gio::Cancellable>,
        |_| {},
    );
    {
        let nb = notebook.clone();
        let weak = new_term.downgrade();
        new_term.connect_child_exited(move |_, _| {
            if let Some(t) = weak.upgrade() {
                close_terminal(&t, &nb);
            }
        });
    }

    let paned = gtk4::Paned::new(orientation);

    // Case 1: focused is a direct notebook page child
    if let Some(idx) = notebook.page_num(&focused_w) {
        let label = notebook.tab_label(&focused_w);
        notebook.remove_page(Some(idx));
        paned.set_start_child(Some(&focused));
        paned.set_end_child(Some(&new_term));
        notebook.insert_page(&paned, label.as_ref(), Some(idx));
        notebook.set_current_page(Some(idx));

    // Case 2: focused is inside a Paned
    } else if let Some(pp) = focused_w
        .parent()
        .and_then(|p| p.downcast::<gtk4::Paned>().ok())
    {
        let is_start = same_widget(pp.start_child().as_ref(), &focused_w);
        let pp_pos = pp.position();

        // Use the container API (not unparent) to detach — unparent() is for
        // widget implementations only and triggers a double-free via the
        // container's remove handler.
        if is_start {
            pp.set_start_child(None::<&gtk4::Widget>);
        } else {
            pp.set_end_child(None::<&gtk4::Widget>);
        }

        paned.set_start_child(Some(&focused));
        paned.set_end_child(Some(&new_term));

        if is_start {
            pp.set_start_child(Some(&paned));
        } else {
            pp.set_end_child(Some(&paned));
        }

        // Restore parent divider after GTK's layout pass resets it.
        // idle_add runs after the current event (and any resulting layouts).
        let pp2 = pp.clone();
        glib::idle_add_local(move || {
            pp2.set_position(pp_pos);
            glib::ControlFlow::Break
        });
    }

    // Set new pane to 50/50 after layout so we know the real allocated size.
    let p = paned.clone();
    glib::idle_add_local(move || {
        let sz = match p.orientation() {
            gtk4::Orientation::Horizontal => p.width(),
            _ => p.height(),
        };
        p.set_position(if sz > 0 { sz / 2 } else { 300 });
        glib::ControlFlow::Break
    });

    new_term.grab_focus();
}

pub fn close(window: &gtk4::ApplicationWindow, notebook: &gtk4::Notebook) {
    if let Some(t) = focused_terminal(window) {
        close_terminal(&t, notebook);
    }
}

pub fn close_terminal(terminal: &vte4::Terminal, notebook: &gtk4::Notebook) {
    let term_w: gtk4::Widget = terminal.clone().upcast();

    // Sole pane — close the whole tab
    if let Some(idx) = notebook.page_num(&term_w) {
        notebook.remove_page(Some(idx));
        if notebook.n_pages() == 0 {
            std::process::exit(0);
        }
        return;
    }

    let parent = match term_w
        .parent()
        .and_then(|p| p.downcast::<gtk4::Paned>().ok())
    {
        Some(p) => p,
        None => return,
    };

    let start = parent.start_child();
    let end = parent.end_child();
    let term_is_start = same_widget(start.as_ref(), &term_w);
    let sibling = if term_is_start { end } else { start };

    let sibling = match sibling {
        Some(s) => s,
        None => return,
    };

    // Detach sibling via the container API before re-parenting it.
    if term_is_start {
        parent.set_end_child(None::<&gtk4::Widget>);
    } else {
        parent.set_start_child(None::<&gtk4::Widget>);
    }

    let parent_w: gtk4::Widget = parent.clone().upcast();

    if let Some(idx) = notebook.page_num(&parent_w) {
        let label = notebook.tab_label(&parent_w);
        notebook.remove_page(Some(idx));
        notebook.insert_page(&sibling, label.as_ref(), Some(idx));
        notebook.set_current_page(Some(idx));
    } else if let Some(gp) = parent_w
        .parent()
        .and_then(|p| p.downcast::<gtk4::Paned>().ok())
    {
        if same_widget(gp.start_child().as_ref(), &parent_w) {
            gp.set_start_child(Some(&sibling));
        } else {
            gp.set_end_child(Some(&sibling));
        }
    }

    focus_first_terminal(&sibling);
}

pub fn navigate(window: &gtk4::ApplicationWindow, notebook: &gtk4::Notebook, forward: bool) {
    let focused = match focused_terminal(window) {
        Some(t) => t,
        None => return,
    };
    let root = match notebook.nth_page(Some(notebook.current_page().unwrap_or(0))) {
        Some(r) => r,
        None => return,
    };

    let terminals = collect_terminals(&root);
    if terminals.len() <= 1 {
        return;
    }

    let fw: gtk4::Widget = focused.clone().upcast();
    let pos = terminals
        .iter()
        .position(|t| same_widget(Some(&t.clone().upcast()), &fw));

    if let Some(i) = pos {
        let next = if forward {
            (i + 1) % terminals.len()
        } else if i == 0 {
            terminals.len() - 1
        } else {
            i - 1
        };
        terminals[next].grab_focus();
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn focused_terminal(window: &gtk4::ApplicationWindow) -> Option<vte4::Terminal> {
    gtk4::prelude::GtkWindowExt::focus(window)?
        .downcast::<vte4::Terminal>()
        .ok()
}

fn same_widget(a: Option<&gtk4::Widget>, b: &gtk4::Widget) -> bool {
    a.map(|w| w.as_ptr() == b.as_ptr()).unwrap_or(false)
}

fn focus_first_terminal(widget: &gtk4::Widget) {
    if let Ok(t) = widget.clone().downcast::<vte4::Terminal>() {
        t.grab_focus();
    } else if let Ok(p) = widget.clone().downcast::<gtk4::Paned>() {
        if let Some(s) = p.start_child() {
            focus_first_terminal(&s);
        }
    }
}

pub fn collect_terminals_pub(widget: &gtk4::Widget) -> Vec<vte4::Terminal> {
    collect_terminals(widget)
}

fn collect_terminals(widget: &gtk4::Widget) -> Vec<vte4::Terminal> {
    if let Ok(t) = widget.clone().downcast::<vte4::Terminal>() {
        return vec![t];
    }
    if let Ok(p) = widget.clone().downcast::<gtk4::Paned>() {
        let mut out = vec![];
        if let Some(s) = p.start_child() {
            out.extend(collect_terminals(&s));
        }
        if let Some(e) = p.end_child() {
            out.extend(collect_terminals(&e));
        }
        return out;
    }
    vec![]
}
