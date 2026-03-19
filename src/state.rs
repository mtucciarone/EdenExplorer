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

#[derive(Clone, Debug)]
pub enum FileOp {
    Rename { from: PathBuf, to: PathBuf },
    Delete { path: PathBuf, backup: PathBuf },
    Create { path: PathBuf },
    Move { from: PathBuf, to: PathBuf },
}

pub struct History {
    pub undo_stack: Vec<FileOp>,
    pub redo_stack: Vec<FileOp>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }
}

fn apply_op(op: &FileOp) -> std::io::Result<()> {
    match op {
        FileOp::Rename { from, to } => std::fs::rename(from, to),

        FileOp::Move { from, to } => std::fs::rename(from, to),

        FileOp::Create { path } => {
            if path.extension().is_none() {
                std::fs::create_dir(path)
            } else {
                std::fs::File::create(path).map(|_| ())
            }
        }

        FileOp::Delete { path, backup } => {
            // Move instead of delete (undo-safe)
            std::fs::rename(path, backup)
        }
    }
}

fn reverse_op(op: &FileOp) -> Option<FileOp> {
    match op {
        FileOp::Rename { from, to } => Some(FileOp::Rename {
            from: to.clone(),
            to: from.clone(),
        }),

        FileOp::Move { from, to } => Some(FileOp::Move {
            from: to.clone(),
            to: from.clone(),
        }),

        // Undo create = delete (move to temp backup)
        FileOp::Create { path } => {
            let backup = std::env::temp_dir().join(
                format!(
                    "{}_undo",
                    path.file_name()?.to_string_lossy()
                )
            );

            Some(FileOp::Delete {
                path: path.clone(),
                backup,
            })
        }

        FileOp::Delete { path, backup } => Some(FileOp::Move {
            from: backup.clone(),
            to: path.clone(),
        }),
    }
}

pub fn execute_op(history: &mut History, op: FileOp) {
    match apply_op(&op) {
        Ok(_) => {
            println!("[EXECUTE ✅] {:?}", op);
            history.undo_stack.push(op);
            history.redo_stack.clear();
        }
        Err(e) => {
            eprintln!("[EXECUTE ❌] {:?} failed: {}", op, e);
        }
    }
}

pub fn undo(history: &mut History) {
    if let Some(op) = history.undo_stack.pop() {
        println!("[UNDO ⏪] {:?}", op);

        if let Some(reverse) = reverse_op(&op) {
            match apply_op(&reverse) {
                Ok(_) => {
                    println!("[UNDO ✅] Applied reverse: {:?}", reverse);
                    history.redo_stack.push(op);
                }
                Err(e) => {
                    eprintln!(
                        "[UNDO ❌] Failed to apply reverse {:?}: {}",
                        reverse, e
                    );

                    // push back so we don't lose history on failure
                    history.undo_stack.push(op);
                }
            }
        } else {
            eprintln!("[UNDO ⚠️] No reverse operation for {:?}", op);

            // push back since we couldn't undo
            history.undo_stack.push(op);
        }
    }
}

pub fn redo(history: &mut History) {
    if let Some(op) = history.redo_stack.pop() {
        println!("[REDO ⏩] {:?}", op);

        match apply_op(&op) {
            Ok(_) => {
                println!("[REDO ✅] {:?}", op);
                history.undo_stack.push(op);
            }
            Err(e) => {
                eprintln!("[REDO ❌] {:?} failed: {}", op, e);

                // push back so redo isn't lost
                history.redo_stack.push(op);
            }
        }
    }
}