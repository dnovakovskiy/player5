//! Ableton Link clock source (WIP).

/// Placeholder.
pub fn probe() -> f64 {
    let link = rusty_link::AblLink::new(120.0);
    let mut s = rusty_link::SessionState::new();
    link.capture_app_session_state(&mut s);
    s.tempo()
}
