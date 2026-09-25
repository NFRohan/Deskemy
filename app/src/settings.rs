//! Settings — preferences saved to config.json — as the Tauri app's
//! routes/settings.

use crate::course_panel::model;
use crate::{AppWindow, Playback, Prefs, SelectOption, Theme};
use deskemy_core::config::AppConfig;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

/// The one config, shared by the pages and the player (which reads the
/// default speed and autoplay as lectures open and end).
pub type Config = Arc<Mutex<AppConfig>>;

pub fn shared(config: AppConfig) -> Config {
    Arc::new(Mutex::new(config))
}

pub fn lock(config: &Config) -> MutexGuard<'_, AppConfig> {
    config.lock().unwrap_or_else(|e| e.into_inner())
}

pub const THEMES: [(&str, &str); 3] = [("dark", "Dark"), ("light", "Light"), ("system", "System")];
pub const SPEEDS: [f64; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
pub const GOALS: [i64; 6] = [15, 30, 45, 60, 90, 120];

fn speed_label(speed: f64) -> String {
    format!("{speed}×")
}

fn goal_label(minutes: i64) -> String {
    format!("{minutes} min")
}

/// Apply one change from the page to a config. Returns false for an unknown
/// key or a value that doesn't parse, leaving the config as it was.
pub fn apply(config: &mut AppConfig, key: &str, value: &str) -> bool {
    let on = value == "true";
    match key {
        "theme" if THEMES.iter().any(|(v, _)| *v == value) => config.theme = value.into(),
        "speed" => match value.parse::<f64>() {
            Ok(speed) if speed > 0.0 => config.default_speed = speed,
            _ => return false,
        },
        "goal" => match value.parse::<i64>() {
            Ok(minutes) if minutes > 0 => config.daily_goal_minutes = minutes,
            _ => return false,
        },
        "autoplay" => config.autoplay_next = on,
        "autohide" => config.autohide_controls = on,
        "clean-titles" => config.clean_titles = on,
        "auto-rescan" => config.auto_rescan = on,
        _ => return false,
    }
    true
}

/// Where config.json lives (None without a data directory: changes then
/// last only for this run).
pub struct SettingsPage {
    config: Config,
    path: Option<PathBuf>,
}

impl SettingsPage {
    pub fn new(config: Config, path: Option<PathBuf>) -> Self {
        SettingsPage { config, path }
    }

    /// Fill the page, and apply what the rest of the UI shows from config.
    pub fn show(&self, ui: &AppWindow) {
        let c = lock(&self.config).clone();
        ui.global::<Theme>().set_mode(c.theme.as_str().into());
        ui.global::<Playback>().set_autohide_fullscreen(c.autohide_controls);

        let prefs = ui.global::<Prefs>();
        let options = |items: Vec<(String, String)>| {
            model(items.into_iter().map(|(value, label)| SelectOption { value: value.into(), label: label.into() }).collect())
        };
        prefs.set_themes(options(THEMES.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect()));
        prefs.set_speeds(options(SPEEDS.iter().map(|s| (s.to_string(), speed_label(*s))).collect()));
        prefs.set_goals(options(GOALS.iter().map(|m| (m.to_string(), goal_label(*m))).collect()));

        prefs.set_theme(c.theme.clone().into());
        let theme = THEMES.iter().find(|(v, _)| *v == c.theme).map_or("Dark", |(_, l)| *l);
        prefs.set_theme_label(theme.into());
        prefs.set_speed(c.default_speed.to_string().into());
        prefs.set_speed_label(speed_label(c.default_speed).into());
        prefs.set_goal(c.daily_goal_minutes.to_string().into());
        prefs.set_goal_label(goal_label(c.daily_goal_minutes).into());
        prefs.set_autoplay(c.autoplay_next);
        prefs.set_autohide(c.autohide_controls);
        prefs.set_clean_titles(c.clean_titles);
        prefs.set_auto_rescan(c.auto_rescan);
    }

    /// Change a setting, save it, and redraw.
    pub fn change(&self, ui: &AppWindow, key: &str, value: &str) {
        let saved = {
            let mut c = lock(&self.config);
            if !apply(&mut c, key, value) {
                tracing::warn!(key, value, "unknown setting");
                return;
            }
            match &self.path {
                Some(path) => c.save(path).map_err(|e| e.to_string()),
                None => Ok(()),
            }
        };
        if let Err(e) = saved {
            tracing::warn!(error = %e, "save config");
        }
        self.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_known_settings_and_rejects_the_rest() {
        let mut c = AppConfig::default();
        assert!(apply(&mut c, "theme", "system"));
        assert!(apply(&mut c, "speed", "1.25"));
        assert!(apply(&mut c, "goal", "45"));
        assert!(apply(&mut c, "autoplay", "false"));
        assert!(apply(&mut c, "autohide", "true"));
        assert_eq!(
            (c.theme.as_str(), c.default_speed, c.daily_goal_minutes, c.autoplay_next, c.autohide_controls),
            ("system", 1.25, 45, false, true)
        );

        let before = format!("{c:?}");
        assert!(!apply(&mut c, "theme", "neon"));
        assert!(!apply(&mut c, "speed", "fast"));
        assert!(!apply(&mut c, "speed", "0"));
        assert!(!apply(&mut c, "goal", "-5"));
        assert!(!apply(&mut c, "volume", "11"));
        assert_eq!(format!("{c:?}"), before, "rejected changes leave it alone");
    }

    #[test]
    fn labels_match_the_tauri_page() {
        assert_eq!(speed_label(1.0), "1×");
        assert_eq!(speed_label(0.75), "0.75×");
        assert_eq!(goal_label(90), "90 min");
    }
}
