use crate::core::fs::{
    MY_PC_PATH, MY_RECYCLE_BIN_PATH, SETTINGS_PATH, parse_search_view_path, parse_tag_view_path,
};
use crate::core::network;
use crate::gui::windows::structs::Navigation;
use std::path::PathBuf;

/// Parent to fall back to when a `PathBuf` has none of its own: a UNC share root's
/// virtual parent is its host's share list, otherwise it's the "This PC" root.
fn root_fallback_parent(current: &std::path::Path) -> PathBuf {
    match network::unc_share_root_host(current) {
        Some(host) => PathBuf::from(format!(r"\\{host}")),
        None => PathBuf::from(MY_PC_PATH),
    }
}

impl Navigation {
    pub fn new(start: PathBuf) -> Self {
        Self {
            current: start,
            back: Vec::new(),
            forward: Vec::new(),
        }
    }

    pub fn go_to(&mut self, path: PathBuf) {
        let target = if path.as_os_str().is_empty() {
            PathBuf::from(MY_PC_PATH)
        } else {
            path
        };

        if self.current != target {
            self.back.push(self.current.clone());
            self.current = target;
            self.forward.clear();
        }
    }

    pub fn is_recycle_bin(&self) -> bool {
        self.current.to_string_lossy() == MY_RECYCLE_BIN_PATH
    }

    pub fn is_settings(&self) -> bool {
        self.current.to_string_lossy() == SETTINGS_PATH
    }

    /// Whether we're showing the virtual "tagged items" list for a tag group,
    /// rather than a real filesystem directory.
    pub fn is_tag_view(&self) -> bool {
        parse_tag_view_path(&self.current).is_some()
    }

    /// Whether we're showing Everything search results, rather than a real
    /// filesystem directory.
    pub fn is_search_view(&self) -> bool {
        parse_search_view_path(&self.current).is_some()
    }

    /// Get the parent directory of the current path
    pub fn get_parent(&self) -> Option<PathBuf> {
        if self.current.to_string_lossy() == MY_PC_PATH {
            return None;
        }

        if self.is_recycle_bin() || self.is_settings() || self.is_tag_view() || self.is_search_view() {
            return None;
        }

        if let Some(parent) = self.current.parent() {
            if parent.as_os_str().is_empty() {
                Some(PathBuf::from(MY_PC_PATH))
            } else {
                Some(parent.to_path_buf())
            }
        } else {
            // Drive root (e.g., "C:\\") or UNC share root (e.g., "\\server\share") has no
            // parent in PathBuf.
            Some(root_fallback_parent(&self.current))
        }
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.back.pop() {
            self.forward.push(self.current.clone());
            self.current = prev;
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            self.back.push(self.current.clone());
            self.current = next;
        }
    }

    pub fn go_up(&mut self) {
        // Prevent breaking virtual root
        if self.current.to_string_lossy() == MY_PC_PATH {
            return;
        }

        if self.is_recycle_bin() || self.is_settings() || self.is_tag_view() || self.is_search_view() {
            return;
        }

        if self.current.as_os_str().is_empty() || !self.current.is_absolute() {
            self.go_to(PathBuf::from(MY_PC_PATH));
            return;
        }

        if let Some(parent) = self.current.parent() {
            if parent.as_os_str().is_empty() {
                self.go_to(PathBuf::from(MY_PC_PATH));
            } else {
                self.go_to(parent.to_path_buf());
            }
        } else {
            // Drive root (e.g., "C:\\") or UNC share root (e.g., "\\server\share") has no
            // parent in PathBuf.
            self.go_to(root_fallback_parent(&self.current));
        }
    }

    /// Helper: are we at virtual root?
    pub fn is_root(&self) -> bool {
        self.current.to_string_lossy() == MY_PC_PATH
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}
