//! App bar auto-hide (SPEC §3) and the Zen reveal rule, as a pure state machine.

use std::time::{Duration, Instant};

/// What the bar needs to know about this frame.
pub struct BarInput {
    pub now: Instant,
    pub scroll_y: f32,
    /// The user is scrolling (wheel, keys, scrollbar), not the app.
    pub user_scrolling: bool,
    /// Pointer distance from the top of the window, if it's inside.
    pub pointer_y: Option<f32>,
    pub hovered: bool,
    /// Find bar, popover or menu open.
    pub pinned: bool,
    pub alt: bool,
    pub zen: bool,
}

#[derive(Debug)]
pub struct AutoHide {
    shown: bool,
    down: f32,
    up: f32,
    last_y: Option<f32>,
    near_top_since: Option<Instant>,
}

impl Default for AutoHide {
    fn default() -> Self {
        Self {
            shown: true,
            down: 0.0,
            up: 0.0,
            last_y: None,
            near_top_since: None,
        }
    }
}

const ALWAYS_SHOWN_ABOVE: f32 = 120.0;
const HIDE_AFTER_DOWN: f32 = 64.0;
const SHOW_AFTER_UP: f32 = 24.0;
const TOP_ZONE: f32 = 56.0;
const ZEN_TOP_ZONE: f32 = 8.0;
const TOP_DWELL: Duration = Duration::from_millis(150);

impl AutoHide {
    /// Update for this frame; returns whether the bar should be shown.
    pub fn update(&mut self, f: &BarInput) -> bool {
        let dy = self.last_y.map_or(0.0, |last| f.scroll_y - last);
        self.last_y = Some(f.scroll_y);
        if !f.user_scrolling {
            // Programmatic scrolls (outline, links, find) and pauses break the run.
            self.down = 0.0;
            self.up = 0.0;
        } else if dy > 0.0 {
            self.down += dy;
            self.up = 0.0;
        } else if dy < 0.0 {
            self.up -= dy;
            self.down = 0.0;
        }

        let zone = if f.zen { ZEN_TOP_ZONE } else { TOP_ZONE };
        let near_top = f.pointer_y.is_some_and(|y| y <= zone);
        if !near_top {
            self.near_top_since = None;
        } else if self.near_top_since.is_none() {
            self.near_top_since = Some(f.now);
        }
        let dwelled = self
            .near_top_since
            .is_some_and(|t| f.now.duration_since(t) >= TOP_DWELL);

        if f.zen {
            // Zen: only the very top edge (or a pinned find bar/popover) brings the bar back.
            self.shown = near_top || f.pinned || (self.shown && f.hovered);
            return self.shown;
        }
        if f.scroll_y < ALWAYS_SHOWN_ABOVE
            || self.up >= SHOW_AFTER_UP
            || dwelled
            || f.alt
            || f.pinned
        {
            self.shown = true;
            self.up = 0.0;
        } else if self.down >= HIDE_AFTER_DOWN && !f.hovered {
            self.shown = false;
            self.down = 0.0;
        }
        self.shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(now: Instant, y: f32, user: bool) -> BarInput {
        BarInput {
            now,
            scroll_y: y,
            user_scrolling: user,
            pointer_y: Some(500.0),
            hovered: false,
            pinned: false,
            alt: false,
            zen: false,
        }
    }

    #[test]
    fn hides_after_64px_of_user_scroll_and_returns_after_24_up() {
        let t = Instant::now();
        let mut a = AutoHide::default();
        assert!(a.update(&input(t, 200.0, false)));
        assert!(a.update(&input(t, 230.0, true)), "30 px isn't enough");
        assert!(!a.update(&input(t, 270.0, true)), "70 px hides");
        assert!(!a.update(&input(t, 260.0, true)), "10 px up isn't enough");
        assert!(a.update(&input(t, 240.0, true)), "30 px up shows");
    }

    #[test]
    fn always_shown_near_the_top_and_ignores_programmatic_scrolls() {
        let t = Instant::now();
        let mut a = AutoHide::default();
        assert!(a.update(&input(t, 0.0, true)));
        assert!(a.update(&input(t, 100.0, true)), "under 120 px");
        assert!(a.update(&input(t, 5000.0, false)), "outline jump");
        assert!(a.update(&input(t, 5040.0, true)));
        assert!(!a.update(&input(t, 5080.0, true)));
    }

    #[test]
    fn hover_pin_alt_and_dwell_keep_it() {
        let t = Instant::now();
        let mut a = AutoHide::default();
        a.update(&input(t, 300.0, false));
        let mut f = input(t, 400.0, true);
        f.hovered = true;
        assert!(a.update(&f), "never hides while hovered");
        a.update(&input(t, 500.0, true));
        assert!(!a.update(&input(t, 600.0, true)));
        let mut f = input(t, 600.0, false);
        f.pointer_y = Some(20.0);
        assert!(!a.update(&f), "needs 150 ms at the top");
        f.now = t + Duration::from_millis(200);
        assert!(a.update(&f));
        let mut a = AutoHide::default();
        a.update(&input(t, 300.0, true));
        assert!(!a.update(&input(t, 400.0, true)));
        let mut f = input(t, 400.0, false);
        f.alt = true;
        assert!(a.update(&f));
    }

    #[test]
    fn zen_shows_only_at_the_top_edge() {
        let t = Instant::now();
        let mut a = AutoHide::default();
        let mut f = input(t, 0.0, false);
        f.zen = true;
        assert!(!a.update(&f), "hidden even at scroll 0");
        f.pointer_y = Some(4.0);
        assert!(a.update(&f));
        f.pointer_y = Some(30.0);
        f.hovered = true;
        assert!(a.update(&f), "stays while hovered");
        f.hovered = false;
        assert!(!a.update(&f));
    }
}
