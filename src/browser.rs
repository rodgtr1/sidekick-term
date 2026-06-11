use gtk4::prelude::*;
use webkit6::prelude::*;

pub struct BrowserPanel {
    pub widget: gtk4::Box,
    pub webview: webkit6::WebView,
}

pub fn build() -> BrowserPanel {
    let webview = webkit6::WebView::new();
    webview.set_vexpand(true);
    webview.set_hexpand(true);

    // Web inspector (right-click -> Inspect Element) for previewing local apps.
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
        settings.set_enable_developer_extras(true);
    }

    let back_btn = gtk4::Button::from_icon_name("go-previous-symbolic");
    let fwd_btn = gtk4::Button::from_icon_name("go-next-symbolic");
    let reload_btn = gtk4::Button::from_icon_name("view-refresh-symbolic");

    let url_entry = gtk4::Entry::new();
    url_entry.set_placeholder_text(Some("Search or enter URL…"));
    url_entry.set_hexpand(true);

    for btn in [&back_btn, &fwd_btn, &reload_btn] {
        btn.add_css_class("browser-nav-btn");
    }

    let nav_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    nav_bar.add_css_class("browser-nav");
    nav_bar.set_margin_start(6);
    nav_bar.set_margin_end(6);
    nav_bar.set_margin_top(4);
    nav_bar.set_margin_bottom(4);
    nav_bar.append(&back_btn);
    nav_bar.append(&fwd_btn);
    nav_bar.append(&reload_btn);
    nav_bar.append(&url_entry);

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.add_css_class("browser-panel");
    container.append(&nav_bar);
    container.append(&webview);

    // Navigate on Enter in URL bar
    {
        let wv = webview.clone();
        url_entry.connect_activate(move |e| {
            let mut text = e.text().to_string();
            if text.is_empty() {
                return;
            }
            if !text.contains("://") {
                if text.contains('.') && !text.contains(' ') {
                    text = format!("https://{}", text);
                } else {
                    text = format!("https://www.google.com/search?q={}", urlencoded(&text));
                }
            }
            wv.load_uri(&text);
        });
    }

    // Sync URL bar with current page URI
    {
        let entry = url_entry.clone();
        webview.connect_uri_notify(move |wv| {
            if let Some(uri) = wv.uri() {
                if uri != "about:blank" {
                    entry.set_text(uri.as_str());
                }
            }
        });
    }

    {
        let wv = webview.clone();
        back_btn.connect_clicked(move |_| {
            wv.go_back();
        });
    }
    {
        let wv = webview.clone();
        fwd_btn.connect_clicked(move |_| {
            wv.go_forward();
        });
    }
    {
        let wv = webview.clone();
        reload_btn.connect_clicked(move |_| {
            wv.reload();
        });
    }

    BrowserPanel {
        widget: container,
        webview,
    }
}

fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b' ' => out.push('+'),
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::urlencoded;

    #[test]
    fn encodes_utf8_bytes_not_codepoints() {
        assert_eq!(urlencoded("a b"), "a+b");
        assert_eq!(urlencoded("rust lang"), "rust+lang");
        // € is U+20AC = E2 82 AC in UTF-8
        assert_eq!(urlencoded("€"), "%E2%82%AC");
        assert_eq!(urlencoded("naïve"), "na%C3%AFve");
        assert_eq!(urlencoded("a-_.~z"), "a-_.~z");
    }
}
