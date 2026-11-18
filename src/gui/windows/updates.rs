use crate::core::i18n::I18n;
use crate::gui::theme::ThemePalette;
use crate::gui::windows::containers::structs::{UpdateCheckStatus, UpdateInfo, UpdateWindow};
use eframe::egui;
use serde::Deserialize;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/mtucciarone/EdenExplorer/releases/latest";

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

pub fn start_update_check(settings: &mut UpdateWindow) {
    if matches!(settings.status, UpdateCheckStatus::Checking) {
        settings.open = true;
        return;
    }

    let (result_tx, result_rx) = mpsc::channel();
    settings.open = true;
    settings.status = UpdateCheckStatus::Checking;
    settings.result_rx = Some(result_rx);

    thread::spawn(move || {
        let result = fetch_latest_release().and_then(|release| {
            let latest_version = release.tag_name.trim_start_matches(['v', 'V']).to_string();
            let current_version = env!("CARGO_PKG_VERSION");

            let current = semver::Version::parse(current_version)
                .map_err(|error| format!("Invalid current version: {error}"))?;
            let latest = semver::Version::parse(&latest_version)
                .map_err(|error| format!("Invalid release version: {error}"))?;

            if latest > current {
                Ok(UpdateInfo {
                    version: latest_version,
                    release_url: release.html_url,
                })
            } else {
                Ok(UpdateInfo {
                    version: current_version.to_string(),
                    release_url: String::new(),
                })
            }
        });

        let _ = result_tx.send(result);
    });
}

fn fetch_latest_release() -> Result<GithubRelease, String> {
    ureq::get(LATEST_RELEASE_API)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "EdenExplorer")
        .call()
        .map_err(|error| error.to_string())?
        .into_json::<GithubRelease>()
        .map_err(|error| error.to_string())
}

pub fn draw_update_window(
    i18n: &I18n,
    ctx: &egui::Context,
    settings: &mut UpdateWindow,
    palette: &ThemePalette,
) -> Option<String> {
    if let Some(result) = settings
        .result_rx
        .as_ref()
        .and_then(|receiver| receiver.try_recv().ok())
    {
        settings.result_rx = None;
        settings.status = match result {
            Ok(info) if info.release_url.is_empty() => UpdateCheckStatus::UpToDate {
                version: info.version,
            },
            Ok(info) => UpdateCheckStatus::UpdateAvailable(info),
            Err(error) => UpdateCheckStatus::Failed(error),
        };
    }

    if !settings.open {
        return None;
    }

    if matches!(settings.status, UpdateCheckStatus::Checking) {
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    let mut should_close = false;
    let mut download_url = None;

    egui::Window::new(i18n.tr("check_for_updates"))
        .collapsible(false)
        .resizable(false)
        .fixed_size([380.0, 180.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::popup(&ctx.style_of(ctx.theme()))
                .corner_radius(egui::CornerRadius::same(8)),
        )
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                match &settings.status {
                    UpdateCheckStatus::Idle | UpdateCheckStatus::Checking => {
                        ui.label(i18n.tr("update_checking"));
                    }
                    UpdateCheckStatus::UpToDate { version } => {
                        ui.label(format!("{} ({version})", i18n.tr("update_up_to_date")));
                    }
                    UpdateCheckStatus::UpdateAvailable(info) => {
                        ui.label(format!("{}: {}", i18n.tr("update_available"), info.version));
                        ui.add_space(12.0);
                        if ui.button(i18n.tr("update_download_now")).clicked() {
                            download_url = Some(info.release_url.clone());
                        }
                    }
                    UpdateCheckStatus::Failed(error) => {
                        ui.label(i18n.tr("update_check_failed"));
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(error)
                                .small()
                                .color(palette.text_normal),
                        );
                    }
                }

                ui.add_space(16.0);
                if ui.button(i18n.tr("update_close")).clicked() {
                    should_close = true;
                }
            });
        });

    if should_close {
        settings.open = false;
    }

    download_url
}
