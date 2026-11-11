use super::*;
use eframe::egui::{Context, RawInput};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn requested_delays(ctx: &Context) -> Arc<Mutex<Vec<Duration>>> {
    let delays = Arc::new(Mutex::new(Vec::new()));
    let sink = delays.clone();
    ctx.set_request_repaint_callback(move |info| sink.lock().unwrap().push(info.delay));
    delays
}

#[test]
fn animation_frame_waits_the_full_interval() {
    let ctx = Context::default();
    let delays = requested_delays(&ctx);
    request_animation_frame(&ctx);
    let delays = delays.lock().unwrap();
    assert_eq!(delays.len(), 1);
    assert!(delays[0] + Duration::from_millis(1) >= ANIMATION_FRAME_INTERVAL);
}

#[test]
fn spinner_is_announced_as_a_progress_indicator() {
    let ctx = Context::default();
    ctx.enable_accesskit();
    let output = ctx.run_ui(RawInput::default(), |ui| {
        loading_spinner(ui, 28.0);
    });
    let update = output
        .platform_output
        .accesskit_update
        .expect("no accesskit tree");
    assert!(
        update
            .nodes
            .iter()
            .any(|(_, node)| format!("{:?}", node.role()).contains("ProgressIndicator"))
    );
}

#[test]
fn spinner_does_not_ask_for_an_immediate_repaint() {
    let ctx = Context::default();
    let delays = requested_delays(&ctx);
    let _ = ctx.run_ui(RawInput::default(), |_| {});
    delays.lock().unwrap().clear();
    let _ = ctx.run_ui(RawInput::default(), |ui| {
        loading_spinner(ui, 28.0);
    });
    let delays = delays.lock().unwrap();
    assert!(!delays.is_empty());
    assert!(
        delays
            .iter()
            .all(|d| *d + Duration::from_millis(1) >= ANIMATION_FRAME_INTERVAL)
    );
}

#[test]
fn no_egui_spinner_is_used() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for (line, value) in text.lines().enumerate() {
                    if [
                        concat!(".spin", "ner()"),
                        concat!("Spin", "ner::"),
                        concat!(".anim", "ate(true)"),
                    ]
                    .iter()
                    .any(|p| value.contains(p))
                    {
                        out.push(format!("{}:{}", path.display(), line + 1));
                    }
                }
            }
        }
    }
    walk(&src, &mut offenders);
    assert!(offenders.is_empty(), "offenders: {offenders:?}");
}
