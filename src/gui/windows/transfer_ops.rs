//! The main window's side of Verify After Copy: a finished copy's
//! notification goes back to "in progress" with "Verifying…" while
//! `core::verify` compares every copy with its source, then ends as
//! "Verified N files" or Failed with how many differ.

use crate::core::verify::{VerifyEvent, VerifyHandle, start_verify};
use crate::gui::windows::containers::notifications::FileOpStatus;
use crate::gui::windows::mainwindow::MainWindow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct PendingVerify {
    handle: VerifyHandle,
    /// Last progress shown, in tenths of a percent.
    shown: u32,
}

impl MainWindow {
    pub(crate) fn start_verify(&mut self, id: u64, sources: &[PathBuf], target_dir: &Path, renames: &HashMap<PathBuf, String>) {
        let handle = start_verify(sources.to_vec(), target_dir.to_path_buf(), renames.clone());
        self.notifications_state.reopen_operation(id);
        self.notifications_state.set_detail(id, Some(self.i18n.tr("verify_in_progress")));
        self.notifications_state.attach_cancel_flag(id, handle.cancel_flag());
        self.pending_verifies.insert(id, PendingVerify { handle, shown: 0 });
    }

    pub(crate) fn poll_pending_verifies(&mut self) {
        if self.pending_verifies.is_empty() {
            return;
        }
        let mut finished = Vec::new();
        for (&id, pending) in self.pending_verifies.iter_mut() {
            let mut progress = None;
            while let Ok(event) = pending.handle.rx.try_recv() {
                match event {
                    VerifyEvent::Progress(p) => progress = Some(p),
                    VerifyEvent::Finished(result) => {
                        finished.push((id, result));
                        break;
                    }
                }
            }
            if let Some(p) = progress {
                let tenths = (p * 1000.0) as u32;
                if tenths != pending.shown {
                    pending.shown = tenths;
                    self.notifications_state.set_progress(id, Some(p));
                }
            }
        }
        for (id, result) in finished {
            self.pending_verifies.remove(&id);
            self.notifications_state.detach_cancel_flag(id);
            self.notifications_state.set_progress(id, None);
            let (status, detail) = match result {
                Ok(result) if result.ok() => (
                    FileOpStatus::Completed,
                    format!("{} {} {}", self.i18n.tr("verify_verified"), result.files, self.i18n.tr("verify_files")),
                ),
                Ok(result) => {
                    let bad = result.mismatched.len() + result.missing.len();
                    let first = result
                        .mismatched
                        .iter()
                        .chain(&result.missing)
                        .next()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (FileOpStatus::Failed, format!("{bad} {} ({first}…)", self.i18n.tr("verify_differ")))
                }
                Err(e) if e.is_empty() => (FileOpStatus::Completed, self.i18n.tr("verify_skipped")),
                Err(e) => (FileOpStatus::Failed, e),
            };
            self.notifications_state.set_detail(id, Some(detail));
            self.notifications_state.finish_operation(id, status);
        }
    }
}
