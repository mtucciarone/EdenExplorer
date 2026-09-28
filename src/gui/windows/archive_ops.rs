//! The main window's side of archives: starting extractions (Extract Here,
//! Extract To, extracting items picked while browsing inside an archive)
//! with a notification that shows progress and has Cancel, and opening a
//! file from inside an archive (extracted to a temp folder first). The
//! work itself is `core::extract` and `core::archive_view`.

use crate::core::extract::{ExtractEvent, ExtractHandle, Target, start_extract};
use crate::gui::windows::containers::enums::ExtractChoice;
use crate::gui::windows::containers::notifications::{FileOpKind, FileOpStatus};
use crate::gui::windows::mainwindow::MainWindow;
use crossbeam_channel::Receiver;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct PendingExtract {
    handle: ExtractHandle,
    /// Last progress shown (so the notification isn't touched every event).
    shown: (u32, String),
}

fn file_label(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

impl MainWindow {
    fn pick_extract_folder(&self) -> Option<PathBuf> {
        crate::gui::windows::windowsoverrides::dialog()
            .set_title(self.i18n.tr("extract_to"))
            .set_directory(self.current_nav().current.clone())
            .pick_folder()
    }

    fn begin_extract(&mut self, archive: PathBuf, target: Target, only: Option<Vec<Vec<String>>>, items: usize) {
        let id = self.notifications_state.start_operation(
            FileOpKind::Extract,
            items,
            file_label(&archive),
            self.settings_window.current_settings.auto_open_notification_panel,
        );
        let handle = start_extract(archive, target, only);
        self.notifications_state.attach_cancel_flag(id, handle.cancel_flag());
        self.notifications_state.set_progress(id, Some(0.0));
        self.pending_extracts.insert(id, PendingExtract { handle, shown: (0, String::new()) });
    }

    /// Extract Here / Extract To "name\" / Extract To… for whole archives.
    pub(crate) fn extract_archives(&mut self, archives: Vec<PathBuf>, choice: ExtractChoice) {
        let target = match choice {
            ExtractChoice::Here => Target::Here,
            ExtractChoice::OwnFolder => Target::OwnFolder,
            ExtractChoice::Pick => match self.pick_extract_folder() {
                Some(dir) => Target::FolderIn(dir),
                None => return,
            },
        };
        for archive in archives {
            self.begin_extract(archive, target.clone(), None, 1);
        }
    }

    /// Extracts items picked while browsing inside an archive: into the
    /// archive's own folder (Here) or a chosen one.
    pub(crate) fn extract_archive_entries(&mut self, paths: Vec<PathBuf>, choice: ExtractChoice) {
        let mut by_archive: HashMap<PathBuf, Vec<Vec<String>>> = HashMap::new();
        for path in &paths {
            if let Some((archive, inner)) = crate::core::archive_view::split(path)
                && !inner.is_empty()
                && let Some(parts) = crate::core::extract::safe_parts(&inner.join("/"))
            {
                by_archive.entry(archive).or_default().push(parts);
            }
        }
        if by_archive.is_empty() {
            return;
        }
        let picked = match choice {
            ExtractChoice::Pick => match self.pick_extract_folder() {
                Some(dir) => Some(dir),
                None => return,
            },
            _ => None,
        };
        for (archive, only) in by_archive {
            let dir = picked
                .clone()
                .unwrap_or_else(|| archive.parent().map(Path::to_path_buf).unwrap_or_default());
            let count = only.len();
            self.begin_extract(archive, Target::Into(dir), Some(only), count);
        }
    }

    /// Called once per frame: moves extraction progress into the
    /// notifications and finishes them.
    pub(crate) fn poll_pending_extracts(&mut self) {
        if self.pending_extracts.is_empty() {
            return;
        }
        let mut finished = Vec::new();
        for (&id, pending) in self.pending_extracts.iter_mut() {
            let mut last_progress = None;
            while let Ok(event) = pending.handle.rx.try_recv() {
                match event {
                    ExtractEvent::Progress(p) => last_progress = Some(p),
                    ExtractEvent::Finished(result) => {
                        finished.push((id, result));
                        break;
                    }
                }
            }
            if let Some(p) = last_progress {
                let percent = if p.total > 0 { (p.done * 1000 / p.total.max(1)) as u32 } else { 0 };
                if (percent, &p.current) != (pending.shown.0, &pending.shown.1) {
                    pending.shown = (percent, p.current.clone());
                    self.notifications_state.set_progress(id, Some(percent as f32 / 1000.0));
                    self.notifications_state.set_detail(id, (!p.current.is_empty()).then(|| p.current.clone()));
                }
            }
        }
        let mut refresh = false;
        for (id, result) in finished {
            self.pending_extracts.remove(&id);
            self.notifications_state.detach_cancel_flag(id);
            let (status, detail) = match result {
                Ok(summary) => {
                    // Show the results if they landed in a folder on screen.
                    let side = self.focused_split;
                    let current = self.active_tab().view(side).nav.current.clone();
                    let parent_of_created = summary.created.first().and_then(|p| p.parent()).map(Path::to_path_buf);
                    if parent_of_created.as_deref() == Some(current.as_path())
                        && let Some(first) = summary.created.first()
                    {
                        self.active_tab_mut().view_mut(side).explorer_state.navigation_selection = Some(first.clone());
                        refresh = true;
                    }
                    let files = if summary.files == 1 {
                        self.i18n.tr("extract_one_file")
                    } else {
                        format!("{} {}", summary.files, self.i18n.tr("extract_files"))
                    };
                    (FileOpStatus::Completed, Some(format!("{files} → {}", file_label(&summary.folder))))
                }
                Err(e) if e.is_empty() => (FileOpStatus::Cancelled, None),
                Err(e) => (FileOpStatus::Failed, Some(e)),
            };
            self.notifications_state.set_detail(id, detail);
            self.notifications_state.set_progress(id, None);
            self.notifications_state.finish_operation(id, status);
        }
        if refresh {
            self.load_path();
        }
    }

    /// Opens a file from inside an archive: extracts it to a temp folder
    /// in the background, then opens it with its default program.
    pub(crate) fn open_from_archive(&mut self, path: PathBuf) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send(crate::core::archive_view::extract_for_open(&path));
            crate::gui::windows::windowsoverrides::request_repaint();
        });
        self.pending_archive_opens.push(rx);
    }

    pub(crate) fn poll_archive_opens(&mut self) {
        let mut opened = Vec::new();
        self.pending_archive_opens.retain(|rx| match rx.try_recv() {
            Ok(result) => {
                opened.push(result);
                false
            }
            Err(crossbeam_channel::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for result in opened {
            match result {
                Ok(file) => crate::gui::windows::mainwindow_imp::handle_pending_actions(
                    Some(crate::gui::windows::containers::enums::ItemViewerAction::OpenWithDefault(vec![file])),
                    self,
                ),
                Err(e) => {
                    let id = self.notifications_state.record_finished(
                        FileOpKind::Extract,
                        1,
                        String::new(),
                        FileOpStatus::Failed,
                        false,
                    );
                    self.notifications_state.set_detail(id, Some(e));
                }
            }
        }
    }
}

pub type ArchiveOpen = Receiver<Result<PathBuf, String>>;
