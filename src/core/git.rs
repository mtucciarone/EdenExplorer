//! Git status for the file view: the branch shown in the status bar and the
//! per-file badges (modified, added, untracked, ...). Read with `gix`, a
//! pure-Rust Git implementation, so Git doesn't need to be installed. It is
//! strictly read-only: nothing in the repository is ever written (not even
//! the index refresh `git status` itself does).
//!
//! `GitService` runs one status read per repository at a time on a
//! background thread and keeps the latest result; asking again while one is
//! running just queues a single re-run.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// A file's or folder's state, in increasing order of importance (a folder
/// shows the most important state of anything inside it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GitState {
    Ignored,
    Untracked,
    Renamed,
    Added,
    Deleted,
    Modified,
    Conflict,
}

impl GitState {
    /// The one-letter badge, as in VS Code.
    pub fn letter(self) -> &'static str {
        match self {
            GitState::Ignored => "I",
            GitState::Untracked => "U",
            GitState::Renamed => "R",
            GitState::Added => "A",
            GitState::Deleted => "D",
            GitState::Modified => "M",
            GitState::Conflict => "C",
        }
    }

    /// i18n key of the badge's tooltip.
    pub fn i18n_key(self) -> &'static str {
        match self {
            GitState::Ignored => "git_state_ignored",
            GitState::Untracked => "git_state_untracked",
            GitState::Renamed => "git_state_renamed",
            GitState::Added => "git_state_added",
            GitState::Deleted => "git_state_deleted",
            GitState::Modified => "git_state_modified",
            GitState::Conflict => "git_state_conflict",
        }
    }
}

/// One repository's status at the time it was read.
#[derive(Debug, Default)]
pub struct RepoStatus {
    pub workdir: PathBuf,
    /// The branch name, or the short commit id when HEAD is detached.
    pub branch: String,
    pub detached: bool,
    /// How many files are changed, added, untracked, ... (not ignored).
    pub changed: usize,
    /// Set when the repository couldn't be read.
    pub error: Option<String>,
    /// Repository-relative path (lowercase, `/`) → state. Untracked and
    /// ignored folders appear once, as the folder itself.
    entries: HashMap<String, GitState>,
    /// Folder (same key form) → the most important change inside it.
    folders: HashMap<String, GitState>,
    /// Whether `entries` has any untracked/ignored folders, so lookups know
    /// to check a path's parents.
    has_collapsed: bool,
}

fn key_of(rel: &str) -> String {
    rel.trim_end_matches('/').replace('\\', "/").to_lowercase()
}

impl RepoStatus {
    fn add(&mut self, rel: &str, state: GitState) {
        let key = key_of(rel);
        if key.is_empty() {
            return;
        }
        if rel.ends_with('/') && matches!(state, GitState::Ignored | GitState::Untracked) {
            self.has_collapsed = true;
        }
        if state != GitState::Ignored {
            self.changed += 1;
            // Every parent folder shows that something inside changed.
            let mut parent = key.as_str();
            while let Some(i) = parent.rfind('/') {
                parent = &parent[..i];
                let slot = self.folders.entry(parent.to_string()).or_insert(state);
                *slot = (*slot).max(state);
            }
        }
        let slot = self.entries.entry(key).or_insert(state);
        *slot = (*slot).max(state);
    }

    /// The state of `path` (a file or folder inside this repository), or
    /// `None` when it's unchanged and tracked.
    pub fn state_of(&self, path: &Path, is_dir: bool) -> Option<GitState> {
        let rel = path.strip_prefix(&self.workdir).ok()?;
        let key = key_of(&rel.to_string_lossy());
        if key.is_empty() {
            return None;
        }
        if let Some(state) = self.entries.get(&key) {
            return Some(*state);
        }
        if is_dir && let Some(state) = self.folders.get(&key) {
            return Some(*state);
        }
        if self.has_collapsed {
            let mut parent = key.as_str();
            while let Some(i) = parent.rfind('/') {
                parent = &parent[..i];
                if let Some(state @ (GitState::Ignored | GitState::Untracked)) =
                    self.entries.get(parent)
                {
                    return Some(*state);
                }
            }
        }
        None
    }
}

/// The root of the Git working tree `dir` is in, if any: the nearest folder
/// (from `dir` up) holding a `.git` folder or file (worktrees and
/// submodules use a file). Network paths are skipped, as probing every
/// parent over the network would slow down browsing.
pub fn find_workdir(dir: &Path) -> Option<PathBuf> {
    if !dir.is_absolute() || dir.to_string_lossy().starts_with(r"\\") {
        return None;
    }
    dir.ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Reads `workdir`'s branch and status (slow for big repositories: run it
/// off the UI thread).
pub fn read_status(workdir: &Path) -> RepoStatus {
    let mut status = RepoStatus {
        workdir: workdir.to_path_buf(),
        ..Default::default()
    };
    if let Err(e) = fill_status(workdir, &mut status) {
        status.error = Some(e);
    }
    status
}

fn fill_status(workdir: &Path, out: &mut RepoStatus) -> Result<(), String> {
    use gix::status::Item;
    use gix::status::index_worktree::Item as Worktree;

    let repo = gix::open(workdir).map_err(|e| e.to_string())?;
    match repo.head_name().map_err(|e| e.to_string())? {
        Some(name) => out.branch = name.shorten().to_string(),
        None => {
            out.detached = true;
            out.branch = repo
                .head_id()
                .map(|id| id.to_hex_with_len(7).to_string())
                .unwrap_or_else(|_| "HEAD".into());
        }
    }

    let iter = repo
        .status(gix::progress::Discard)
        .map_err(|e| e.to_string())?
        .untracked_files(gix::status::UntrackedFiles::Collapsed)
        .index_worktree_submodules(None)
        .dirwalk_options(|o| o.emit_ignored(Some(gix::dir::walk::EmissionMode::CollapseDirectory)))
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|e| e.to_string())?;
    for item in iter {
        let Ok(item) = item else { continue };
        match &item {
            // Staged changes (HEAD to index).
            Item::TreeIndex(change) => {
                use gix::diff::index::ChangeRef;
                let state = match change {
                    ChangeRef::Addition { .. } => GitState::Added,
                    ChangeRef::Deletion { .. } => GitState::Deleted,
                    ChangeRef::Modification { .. } => GitState::Modified,
                    ChangeRef::Rewrite { .. } => GitState::Renamed,
                };
                out.add(&change.location().to_string(), state);
            }
            // Unstaged changes, untracked and ignored files.
            Item::IndexWorktree(change) => {
                let rel = change.rela_path().to_string();
                let state = match change {
                    Worktree::DirectoryContents { entry, .. } => match entry.status {
                        gix::dir::entry::Status::Untracked => GitState::Untracked,
                        gix::dir::entry::Status::Ignored(_) => GitState::Ignored,
                        _ => continue,
                    },
                    _ => {
                        use gix::status::index_worktree::iter::Summary;
                        match change.summary() {
                            Some(Summary::Removed) => GitState::Deleted,
                            Some(Summary::Added | Summary::IntentToAdd) => GitState::Added,
                            Some(Summary::Modified | Summary::TypeChange) => GitState::Modified,
                            Some(Summary::Renamed | Summary::Copied) => GitState::Renamed,
                            Some(Summary::Conflict) => GitState::Conflict,
                            None => continue,
                        }
                    }
                };
                // A collapsed folder keeps its trailing slash.
                let is_dir = matches!(change, Worktree::DirectoryContents { entry, .. }
                    if entry.disk_kind.is_some_and(|k| k.is_dir()));
                let rel = if is_dir && !rel.ends_with('/') {
                    format!("{rel}/")
                } else {
                    rel
                };
                out.add(&rel, state);
            }
        }
    }
    Ok(())
}

struct RepoEntry {
    status: Option<Arc<RepoStatus>>,
    running: bool,
    /// Asked again while running: read once more when it finishes.
    again: bool,
}

/// Keeps every visited repository's latest status, reading them in the
/// background (see the module docs).
pub struct GitService {
    repos: HashMap<PathBuf, RepoEntry>,
    tx: Sender<RepoStatus>,
    rx: Receiver<RepoStatus>,
}

impl Default for GitService {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            repos: HashMap::new(),
            tx,
            rx,
        }
    }
}

impl GitService {
    /// Reads `workdir`'s status again (or once more after the read in
    /// progress). `wake` is called when it's done.
    pub fn request(&mut self, workdir: &Path, wake: impl Fn() + Send + 'static) {
        let entry = self
            .repos
            .entry(workdir.to_path_buf())
            .or_insert(RepoEntry {
                status: None,
                running: false,
                again: false,
            });
        if entry.running {
            entry.again = true;
            return;
        }
        entry.running = true;
        let (tx, workdir) = (self.tx.clone(), workdir.to_path_buf());
        std::thread::spawn(move || {
            let _ = tx.send(read_status(&workdir));
            wake();
        });
    }

    /// Picks up finished reads; returns the repositories to read again.
    pub fn poll(&mut self) -> Vec<PathBuf> {
        let mut again = Vec::new();
        while let Ok(status) = self.rx.try_recv() {
            if let Some(entry) = self.repos.get_mut(&status.workdir) {
                entry.running = false;
                if std::mem::take(&mut entry.again) {
                    again.push(status.workdir.clone());
                }
                entry.status = Some(Arc::new(status));
            }
        }
        again
    }

    pub fn status(&self, workdir: &Path) -> Option<Arc<RepoStatus>> {
        self.repos.get(workdir).and_then(|e| e.status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_roll_up_to_folders_and_collapsed_folders_cover_their_contents() {
        let mut s = RepoStatus {
            workdir: PathBuf::from(r"C:\repo"),
            ..Default::default()
        };
        s.add("src/main.rs", GitState::Modified);
        s.add("src/new.rs", GitState::Untracked);
        s.add("docs/guide.md", GitState::Added);
        s.add("target/", GitState::Ignored);
        s.add("scratch/", GitState::Untracked);
        let at = |p: &str, dir: bool| s.state_of(&Path::new(r"C:\repo").join(p), dir);
        assert_eq!(at(r"src\main.rs", false), Some(GitState::Modified));
        assert_eq!(at(r"SRC\Main.rs", false), Some(GitState::Modified));
        assert_eq!(at("src", true), Some(GitState::Modified));
        assert_eq!(at("docs", true), Some(GitState::Added));
        assert_eq!(at(r"src\lib.rs", false), None);
        assert_eq!(at("target", true), Some(GitState::Ignored));
        assert_eq!(at(r"target\debug\app.exe", false), Some(GitState::Ignored));
        assert_eq!(at(r"scratch\notes.txt", false), Some(GitState::Untracked));
        assert_eq!(at("README.md", false), None);
        // Ignored files don't count as changes.
        assert_eq!(s.changed, 4);
    }
}
