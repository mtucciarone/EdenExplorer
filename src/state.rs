use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct FileItem {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub file_size: Option<u64>,
    pub modified_time: Option<String>,
    pub created_time: Option<String>,

    // Optional drive info (only populated for drive roots)
    pub total_space: Option<u64>,
    pub free_space: Option<u64>,
}

impl FileItem {
    pub fn new(
        name: String,
        path: PathBuf,
        is_dir: bool,
        file_size: Option<u64>,
        modified_time: Option<String>,
        created_time: Option<String>,
    ) -> Self {
        Self {
            name,
            path,
            is_dir,
            file_size,
            modified_time,
            created_time,
            total_space: None,
            free_space: None,
        }
    }

    pub fn with_drive_info(
        name: String,
        path: PathBuf,
        is_dir: bool,
        file_size: Option<u64>,
        modified_time: Option<String>,
        created_time: Option<String>,
        total: u64,
        free: u64,
    ) -> Self {
        Self {
            name,
            path,
            is_dir,
            file_size,
            modified_time,
            created_time,
            total_space: Some(total),
            free_space: Some(free),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Navigation {
    pub current: PathBuf,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
}

impl Navigation {
    pub fn new() -> Self {
        Self {
            current: PathBuf::from("::MY_PC::"),
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
        if self.current.to_string_lossy() == "::MY_PC::" {
            return;
        }

        if let Some(parent) = self.current.parent() {
            self.go_to(parent.to_path_buf());
        } else {
            // Drive root (e.g., "C:\\") has no parent in PathBuf.
            self.go_to(PathBuf::from("::MY_PC::"));
        }
    }

    /// Helper: are we at virtual root?
    pub fn is_root(&self) -> bool {
        self.current.to_string_lossy() == "::MY_PC::"
    }
}
