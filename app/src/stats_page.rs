//! The Stats page — today's goal and streak, lifetime totals, the activity
//! heatmap and this week's chart — as the Tauri app's routes/stats.

use crate::course_panel::model;
use crate::library::{format_duration, pct};
use crate::session::Db;
use crate::{AppWindow, LearningStats, WeekBar};
use chrono::{Datelike, Duration, Local, NaiveDate};
use deskemy_core::db::queries;
use deskemy_core::domain::{DayActivity, LibraryStats};
use slint::ComponentHandle;
use std::collections::HashMap;

/// Columns in the heatmap (a week each, Sunday at the top).
pub const WEEKS: i64 = 26;

/// A day's heatmap shade, 0 (nothing) to 4 (an hour or more).
pub fn level(seconds: f64) -> i32 {
    let minutes = seconds / 60.0;
    match minutes {
        m if m <= 0.0 => 0,
        m if m < 15.0 => 1,
        m if m < 30.0 => 2,
        m if m < 60.0 => 3,
        _ => 4,
    }
}

fn by_day(activity: &[DayActivity]) -> HashMap<&str, f64> {
    activity.iter().map(|a| (a.day.as_str(), a.watch_seconds)).collect()
}

fn key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// The heatmap's cells, column by column (oldest week first, Sunday to
/// Saturday), ending with the week holding `today`; days after today are -1.
pub fn heatmap(today: NaiveDate, activity: &[DayActivity]) -> Vec<i32> {
    let seconds = by_day(activity);
    let last_saturday = today + Duration::days(6 - today.weekday().num_days_from_sunday() as i64);
    let first_sunday = last_saturday - Duration::days(WEEKS * 7 - 1);
    (0..WEEKS * 7)
        .map(|i| {
            let date = first_sunday + Duration::days(i);
            if date > today {
                -1
            } else {
                level(seconds.get(key(date).as_str()).copied().unwrap_or(0.0))
            }
        })
        .collect()
}

/// The last seven days, oldest first: (weekday initial, seconds watched).
pub fn week(today: NaiveDate, activity: &[DayActivity]) -> Vec<(&'static str, f64)> {
    const INITIALS: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];
    let seconds = by_day(activity);
    (0..7)
        .rev()
        .map(|back| {
            let date = today - Duration::days(back);
            let initial = INITIALS[date.weekday().num_days_from_sunday() as usize];
            (initial, seconds.get(key(date).as_str()).copied().unwrap_or(0.0))
        })
        .collect()
}

/// Today's share of the daily goal, 0-100.
pub fn goal_percent(today_minutes: i64, goal_minutes: i64) -> i32 {
    if goal_minutes <= 0 {
        return 0;
    }
    ((today_minutes as f64 / goal_minutes as f64 * 100.0).round() as i32).min(100)
}

/// Lifetime watch time: content watched, else the sum of logged activity.
pub fn total_watch(stats: &LibraryStats) -> f64 {
    if stats.watched_seconds > 0.0 {
        stats.watched_seconds
    } else {
        stats.activity.iter().map(|d| d.watch_seconds).sum()
    }
}

/// The goal ring's arc as path commands in an 80×80 box (radius 34, from
/// twelve o'clock, clockwise); "" at 0%.
pub fn ring(percent: i32) -> String {
    const C: f64 = 40.0;
    const R: f64 = 34.0;
    let percent = percent.clamp(0, 100);
    if percent == 0 {
        return String::new();
    }
    if percent == 100 {
        // An arc can't end where it starts: draw the circle as two halves.
        return format!("M {C} {} A {R} {R} 0 1 1 {C} {} A {R} {R} 0 1 1 {C} {}", C - R, C + R, C - R);
    }
    let angle = percent as f64 / 100.0 * std::f64::consts::TAU;
    let (x, y) = (C + R * angle.sin(), C - R * angle.cos());
    let large = if percent > 50 { 1 } else { 0 };
    format!("M {C} {} A {R} {R} 0 {large} 1 {x:.2} {y:.2}", C - R)
}

fn days(n: i64) -> &'static str {
    if n == 1 { "day" } else { "days" }
}

/// Load the stats and show them.
pub fn show(ui: &AppWindow, db: &Db, goal_minutes: i64) {
    let stats = {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        queries::stats(&conn)
    };
    let page = ui.global::<LearningStats>();
    let mut stats = match stats {
        Ok(stats) => stats,
        Err(e) => {
            tracing::warn!(error = %e, "stats");
            page.set_error(e.to_string().into());
            return;
        }
    };
    stats.daily_goal_minutes = goal_minutes;
    page.set_error("".into());

    let today = Local::now().date_naive();
    let today_minutes = (stats.watch_seconds_today / 60.0).round() as i64;
    let goal = goal_percent(today_minutes, stats.daily_goal_minutes);
    page.set_goal_percent(goal);
    page.set_goal_ring(ring(goal).into());
    page.set_goal_text(format!("{today_minutes} / {} min", stats.daily_goal_minutes).into());
    page.set_streak(stats.current_streak as i32);
    page.set_streak_unit(days(stats.current_streak).into());
    page.set_best_streak(stats.best_streak as i32);
    page.set_focus_id(stats.focus_course_id.clone().unwrap_or_default().into());
    page.set_focus_title(stats.focus_course_title.clone().unwrap_or_default().into());
    page.set_focus_percent(stats.focus_course_pct as i32);

    page.set_watch_total(format_duration(Some(total_watch(&stats))).into());
    page.set_today_minutes(today_minutes as i32);
    page.set_lectures_completed(stats.lectures_completed as i32);
    page.set_lectures_total(stats.lectures_total as i32);
    page.set_lecture_percent(pct(stats.lectures_completed, stats.lectures_total));
    page.set_courses_completed(stats.courses_completed as i32);
    page.set_courses_total(stats.courses_total as i32);

    page.set_active_days_month(stats.active_days_month as i32);
    page.set_heatmap(model(heatmap(today, &stats.activity)));
    let week = week(today, &stats.activity);
    let most = week.iter().map(|(_, s)| *s).fold(1.0, f64::max);
    let last = week.len() - 1;
    page.set_week(model(
        week.into_iter()
            .enumerate()
            .map(|(i, (label, seconds))| WeekBar {
                label: label.into(),
                fraction: (seconds / most).max(0.04) as f32,
                today: i == last,
            })
            .collect(),
    ));
    page.set_week_total(format_duration(Some(stats.watch_seconds_week)).into());

    page.set_in_progress(stats.courses_in_progress as i32);
    page.set_bookmarks(stats.bookmarks_total as i32);
    page.set_lectures_last_7(stats.lectures_last_7 as i32);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str, minutes: f64) -> DayActivity {
        DayActivity { day: date.into(), watch_seconds: minutes * 60.0, lectures_completed: 0 }
    }

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn levels_step_at_15_30_and_60_minutes() {
        let levels: Vec<i32> = [0.0, 1.0, 15.0, 30.0, 59.0, 60.0, 300.0].iter().map(|m| level(m * 60.0)).collect();
        assert_eq!(levels, [0, 1, 2, 3, 3, 4, 4]);
    }

    #[test]
    fn heatmap_ends_on_this_weeks_saturday() {
        // 2026-09-25 is a Friday: the last cell (Saturday) is in the future.
        let today = date("2026-09-25");
        let activity = [day("2026-09-25", 45.0), day("2026-09-20", 5.0), day("2026-03-29", 90.0)];
        let cells = heatmap(today, &activity);
        assert_eq!(cells.len(), (WEEKS * 7) as usize);
        assert_eq!(cells[cells.len() - 1], -1);
        assert_eq!(cells[cells.len() - 2], 3);
        // Sunday of this week.
        assert_eq!(cells[cells.len() - 7], 1);
        // The first cell is the Sunday 25 weeks before this week's.
        assert_eq!(cells[0], 4);
        assert_eq!(cells.iter().filter(|&&c| c == -1).count(), 1);
    }

    #[test]
    fn week_runs_oldest_to_today() {
        let today = date("2026-09-25");
        let w = week(today, &[day("2026-09-25", 10.0), day("2026-09-19", 2.0)]);
        let labels: Vec<&str> = w.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["S", "S", "M", "T", "W", "T", "F"]);
        assert_eq!(w[0].1, 120.0);
        assert_eq!(w[6].1, 600.0);
    }

    #[test]
    fn goal_percent_caps_at_100() {
        assert_eq!(goal_percent(15, 30), 50);
        assert_eq!(goal_percent(90, 30), 100);
        assert_eq!(goal_percent(10, 0), 0);
    }

    #[test]
    fn ring_arcs_clockwise_from_the_top() {
        assert_eq!(ring(0), "");
        assert_eq!(ring(25), "M 40 6 A 34 34 0 0 1 74.00 40.00");
        assert_eq!(ring(75), "M 40 6 A 34 34 0 1 1 6.00 40.00");
        assert!(ring(100).matches(" A ").count() == 2);
    }
}
