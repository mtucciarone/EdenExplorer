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
        if self.current != path {
            self.back.push(self.current.clone());
            self.current = path;
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

        if let Some(parent) = self.current.parent() {
            self.go_to(parent.to_path_buf());
        } else {
            // Drive root (e.g., "C:\\") has no parent in PathBuf.
            self.go_to(PathBuf::from(MY_PC_PATH));
        }
    }

    /// Helper: are we at virtual root?
    pub fn is_root(&self) -> bool {
        self.current.to_string_lossy() == MY_PC_PATH
    }
}
