//! player_* commands over the embedded mpv player, plus lecture_get for the
//! watch-page header.

use crate::db::queries;
use crate::error::{DeskemyError, Result};
use crate::player::{MediaTracks, MpvPlayer, PlayerService, PlayerState};
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, State};

/// Ensure the player exists (created lazily on first open) and run `f`.
fn ensure<R>(
    app: &AppHandle,
    state: &State<AppState>,
    f: impl FnOnce(&MpvPlayer) -> Result<R>,
) -> Result<R> {
    let mut guard = state
        .player
        .lock()
        .map_err(|_| DeskemyError::Other("player lock poisoned".into()))?;
    if guard.is_none() {
        *guard = Some(MpvPlayer::new(app.clone())?);
    }
    f(guard.as_ref().unwrap())
}

/// Run `f` only if the player already exists (no lazy creation).
fn existing<R: Default>(
    state: &State<AppState>,
    f: impl FnOnce(&MpvPlayer) -> Result<R>,
) -> Result<R> {
    let guard = state
        .player
        .lock()
        .map_err(|_| DeskemyError::Other("player lock poisoned".into()))?;
    match guard.as_ref() {
        Some(p) => f(p),
        None => Ok(R::default()),
    }
}

/// Whether libmpv could be found/loaded — lets the UI prompt to install mpv.
#[tauri::command]
pub fn player_available() -> bool {
    crate::mpv::is_available()
}

#[tauri::command]
pub fn player_open(app: AppHandle, state: State<AppState>, lecture_id: String) -> Result<()> {
    ensure(&app, &state, |p| p.open(&lecture_id))
}

#[tauri::command]
pub fn player_toggle_pause(state: State<AppState>) -> Result<()> {
    existing(&state, |p| p.toggle_pause())
}

#[tauri::command]
pub fn player_set_paused(state: State<AppState>, paused: bool) -> Result<()> {
    existing(&state, |p| p.set_paused(paused))
}

#[tauri::command]
pub fn player_seek(state: State<AppState>, position: f64) -> Result<()> {
    existing(&state, |p| p.seek(position))
}

#[tauri::command]
pub fn player_set_speed(state: State<AppState>, speed: f64) -> Result<()> {
    existing(&state, |p| p.set_speed(speed))
}

#[tauri::command]
pub fn player_next(state: State<AppState>) -> Result<()> {
    existing(&state, |p| p.next())
}

#[tauri::command]
pub fn player_prev(state: State<AppState>) -> Result<()> {
    existing(&state, |p| p.prev())
}

#[tauri::command]
pub fn player_tracks(state: State<AppState>) -> Result<MediaTracks> {
    existing(&state, |p| Ok(p.tracks()))
}

#[tauri::command]
pub fn player_set_subtitle(state: State<AppState>, sid: Option<i64>) -> Result<()> {
    existing(&state, |p| p.set_subtitle(sid))
}

#[tauri::command]
pub fn player_set_audio(state: State<AppState>, aid: Option<i64>) -> Result<()> {
    existing(&state, |p| p.set_audio(aid))
}

#[tauri::command]
pub fn player_set_chapter(state: State<AppState>, index: i64) -> Result<()> {
    existing(&state, |p| p.set_chapter(index))
}

#[tauri::command]
pub fn player_set_rect(
    state: State<AppState>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<()> {
    existing(&state, |p| p.set_rect(x, y, w, h))
}

#[tauri::command]
pub fn player_stop(state: State<AppState>) -> Result<()> {
    existing(&state, |p| p.stop())
}

#[tauri::command]
pub fn player_state(state: State<AppState>) -> Result<Option<PlayerState>> {
    let guard = state
        .player
        .lock()
        .map_err(|_| DeskemyError::Other("player lock poisoned".into()))?;
    Ok(guard.as_ref().map(|p| p.state()))
}

#[derive(Serialize)]
pub struct LectureView {
    pub id: String,
    pub title: String,
    pub course_id: String,
    pub course_title: String,
    pub section_title: String,
}

#[tauri::command]
pub fn lecture_get(state: State<AppState>, id: String) -> Result<Option<LectureView>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| DeskemyError::Other("db lock poisoned".into()))?;
    Ok(queries::get_lecture_view(&conn, &id)?.map(
        |(title, course_id, course_title, section_title)| LectureView {
            id,
            title,
            course_id,
            course_title,
            section_title,
        },
    ))
}
