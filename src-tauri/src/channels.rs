use std::sync::Arc;
use tauri::State;
use postal_core::{ChannelPage, ChannelSummary, ChannelView, WhatsAppService};
use crate::{AppState, command_error::{CommandError, CommandResult}};

fn current(state: &AppState, account_id: &str, service: &Arc<WhatsAppService>) -> CommandResult<()> {
    let active = state.account_service(account_id).map_err(|error|
        CommandError::code("error.account_changed").with_diagnostic(error))?;
    if Arc::ptr_eq(&active, service) { Ok(()) } else { Err(CommandError::code("error.account_changed")) }
}

#[tauri::command]
pub(crate) async fn channels(state: State<'_, AppState>, account_id: String) -> CommandResult<ChannelView> {
    let service = state.account_service(&account_id)?;
    let result = service.channels().await;
    current(&state, &account_id, &service)?;
    result.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn refresh_channels(state: State<'_, AppState>, account_id: String) -> CommandResult<ChannelView> {
    let service = state.account_service(&account_id)?;
    let result = service.refresh_channels().await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn channel_metadata(state: State<'_, AppState>, account_id: String, jid: String) -> CommandResult<ChannelSummary> {
    let service = state.account_service(&account_id)?;
    let result = service.channel_metadata(&jid).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn follow_channel(state: State<'_, AppState>, account_id: String, jid: String) -> CommandResult<ChannelSummary> {
    let service = state.account_service(&account_id)?;
    let result = service.follow_channel(&jid).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn unfollow_channel(state: State<'_, AppState>, account_id: String, jid: String) -> CommandResult<()> {
    let service = state.account_service(&account_id)?;
    let result = service.unfollow_channel(&jid).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn set_channel_muted(state: State<'_, AppState>, account_id: String, jid: String, muted: bool) -> CommandResult<ChannelSummary> {
    let service = state.account_service(&account_id)?;
    let result = service.set_channel_muted(&jid, muted).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn set_channel_favorite(state: State<'_, AppState>, account_id: String, jid: String, favorite: bool) -> CommandResult<ChannelSummary> {
    let service = state.account_service(&account_id)?;
    let result = service.set_channel_favorite(&jid, favorite).await;
    current(&state, &account_id, &service)?;
    result.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn channel_messages(state: State<'_, AppState>, account_id: String, jid: String,
    before: Option<String>, limit: Option<u32>) -> CommandResult<ChannelPage> {
    let service = state.account_service(&account_id)?;
    let result = service.channel_messages(&jid, before.as_deref(), limit.unwrap_or(50)).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}
