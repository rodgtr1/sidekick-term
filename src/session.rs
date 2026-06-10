use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Saved layout of one tab: either a single terminal or a binary split.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    Terminal {
        cwd: String,
    },
    Split {
        /// "h" = side by side, "v" = stacked.
        orientation: String,
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    /// cwd of the left/top-most terminal — the one a restored tab starts with.
    pub fn first_cwd(&self) -> &str {
        match self {
            Node::Terminal { cwd } => cwd,
            Node::Split { first, .. } => first.first_cwd(),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Session {
    #[serde(default)]
    pub tabs: Vec<Node>,
}

pub fn state_path() -> PathBuf {
    let state_dir = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".local/state")
        });
    state_dir.join("sidekick").join("session.json")
}

/// Best-effort save; written via temp-file-and-rename so a crash mid-write
/// never corrupts the previous session.
pub fn save(session: &Session) {
    let path = state_path();
    let Some(dir) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(json) = serde_json::to_string_pretty(session) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

pub fn load() -> Option<Session> {
    let content = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&content).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_roundtrips() {
        let session = Session {
            tabs: vec![
                Node::Terminal { cwd: "/tmp".into() },
                Node::Split {
                    orientation: "h".into(),
                    first: Box::new(Node::Terminal { cwd: "/a".into() }),
                    second: Box::new(Node::Split {
                        orientation: "v".into(),
                        first: Box::new(Node::Terminal { cwd: "/b".into() }),
                        second: Box::new(Node::Terminal { cwd: "/c".into() }),
                    }),
                },
            ],
        };
        let json = serde_json::to_string(&session).unwrap();
        let back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tabs.len(), 2);
        assert_eq!(back.tabs[1].first_cwd(), "/a");
        match &back.tabs[1] {
            Node::Split { second, .. } => assert_eq!(second.first_cwd(), "/b"),
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn empty_or_garbage_session_is_none_or_empty() {
        let s: Session = serde_json::from_str("{}").unwrap();
        assert!(s.tabs.is_empty());
        assert!(serde_json::from_str::<Session>("not json").is_err());
    }
}
