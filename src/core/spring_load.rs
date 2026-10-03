//! Spring-loaded folders: while files are being dragged, resting on a
//! folder for a moment opens it (or, in the sidebar tree, expands it), so
//! the drop can go deeper without letting go. This is just the timer; the
//! file list and the folder tree each feed it whatever folder is under the
//! pointer every frame.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone)]
pub struct SpringLoad {
    target: Option<PathBuf>,
    since: Option<Instant>,
    fired: bool,
}

/// What `SpringLoad::update` decided this frame.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SpringStep {
    /// Open this folder now (reported once per hover).
    pub open: Option<PathBuf>,
    /// How far along the wait is (0–1), for a progress hint; 0 when idle.
    pub progress: f32,
    /// Time left until it opens, to schedule the next repaint.
    pub remaining: Option<Duration>,
}

impl SpringLoad {
    /// `hovered`: the folder under the pointer during a drag (`None` when
    /// not dragging or not over a folder).
    pub fn update(&mut self, hovered: Option<&Path>, now: Instant, delay: Duration) -> SpringStep {
        let Some(hovered) = hovered else {
            *self = SpringLoad::default();
            return SpringStep::default();
        };
        if self.target.as_deref() != Some(hovered) {
            self.target = Some(hovered.to_path_buf());
            self.since = Some(now);
            self.fired = false;
        }
        if self.fired {
            return SpringStep::default();
        }
        let elapsed = now.saturating_duration_since(self.since.unwrap_or(now));
        if elapsed >= delay {
            self.fired = true;
            return SpringStep {
                open: self.target.clone(),
                progress: 1.0,
                remaining: None,
            };
        }
        SpringStep {
            open: None,
            progress: elapsed.as_secs_f32() / delay.as_secs_f32().max(0.001),
            remaining: Some(delay - elapsed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_once_after_resting_on_a_folder() {
        let delay = Duration::from_millis(800);
        let t0 = Instant::now();
        let a = Path::new(r"C:\A");
        let b = Path::new(r"C:\B");
        let mut spring = SpringLoad::default();
        assert_eq!(spring.update(Some(a), t0, delay).open, None);
        let mid = spring.update(Some(a), t0 + Duration::from_millis(400), delay);
        assert!(mid.open.is_none() && (mid.progress - 0.5).abs() < 0.01);
        assert_eq!(mid.remaining, Some(Duration::from_millis(400)));
        assert_eq!(
            spring.update(Some(a), t0 + delay, delay).open.as_deref(),
            Some(a)
        );
        // Only once per hover.
        assert_eq!(spring.update(Some(a), t0 + delay * 2, delay).open, None);
        // Moving to another folder restarts the wait.
        assert_eq!(spring.update(Some(b), t0 + delay * 2, delay).open, None);
        assert_eq!(
            spring
                .update(Some(b), t0 + delay * 3, delay)
                .open
                .as_deref(),
            Some(b)
        );
        // Leaving resets, so coming back waits again.
        assert_eq!(
            spring.update(None, t0 + delay * 3, delay),
            SpringStep::default()
        );
        assert_eq!(spring.update(Some(b), t0 + delay * 3, delay).open, None);
    }
}
