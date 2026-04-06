use crate::core::fs::MY_PC_PATH;
use crate::gui::windows::structs::Navigation;
use std::path::PathBuf;

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
            // Drive root (e.g., "C:\\") has no parent in PathBuf.
            self.go_to(PathBuf::from(MY_PC_PATH));
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
