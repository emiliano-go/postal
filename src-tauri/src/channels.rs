use std::sync::Arc;
use tauri::State;
use postal_core::{ChannelPage, ChannelSummary, ChannelView, WhatsAppService};
use crate::{AppState, command_error::{CommandError, CommandResult}};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

fn current(state: &AppState, account_id: &str, service: &Arc<WhatsAppService>) -> CommandResult<()> {
    let active = state.account_service(account_id).map_err(|error|
        CommandError::code("error.account_changed").with_diagnostic(error))?;
    if Arc::ptr_eq(&active, service) { Ok(()) } else { Err(CommandError::code("error.account_changed")) }
}

fn inline_media(data: &str) -> CommandResult<Vec<u8>> {
    if data.len() > ((1024 * 1024 + 2) / 3) * 4 {
        return Err(CommandError::operation_failed("channel attachments above 1 MiB require a staged upload"));
    }
    let bytes = BASE64.decode(data.as_bytes()).map_err(|error|
        CommandError::code("error.media_base64_invalid").with_diagnostic(error))?;
    if bytes.len() > 1024 * 1024 {
        return Err(CommandError::operation_failed("channel attachments above 1 MiB require a staged upload"));
    }
    Ok(bytes)
}

#[tauri::command]
pub(crate) async fn channel_can_post(state: State<'_, AppState>, account_id: String, jid: String) -> CommandResult<bool> {
    let service = state.account_service(&account_id)?;
    let result = service.channel_can_post(&jid).await;
    current(&state, &account_id, &service)?;
    result.map_err(CommandError::from)
}

#[tauri::command]
pub(crate) async fn channel_post_text(state: State<'_, AppState>, account_id: String, jid: String, text: String) -> CommandResult<()> {
    let service = state.account_service(&account_id)?;
    let result = service.channel_post_text(&jid, &text).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn channel_post_poll(state: State<'_, AppState>, account_id: String, jid: String,
    question: String, options: Vec<String>, multi: bool) -> CommandResult<()> {
    let service = state.account_service(&account_id)?;
    let result = service.channel_post_poll(&jid, &question, options, multi).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn channel_post_media(state: State<'_, AppState>, account_id: String, jid: String,
    name: String, data: Option<String>, upload: Option<String>, caption: Option<String>) -> CommandResult<Option<String>> {
    let service = state.account_service(&account_id)?;
    let result = match (data, upload) {
        (Some(data), None) => service.channel_post_media(&jid, &name, inline_media(&data)?, caption).await,
        (None, Some(token)) => {
            let owner = account_id.clone();
            let uploads = state.uploads.clone();
            let staged = tauri::async_runtime::spawn_blocking(move || uploads.take(&owner, &token))
                .await.map_err(|error| CommandError::operation_failed(error))??;
            current(&state, &account_id, &service)?;
            service.channel_post_media_file(&jid, &staged.name, staged.path.clone(), caption).await
        }
        _ => return Err(CommandError::code("error.media_input_required")),
    };
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn channel_edit_text(state: State<'_, AppState>, account_id: String, jid: String,
    id: String, text: String) -> CommandResult<()> {
    let service = state.account_service(&account_id)?;
    let result = service.channel_edit_text(&jid, &id, &text).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[tauri::command]
pub(crate) async fn channel_revoke_post(state: State<'_, AppState>, account_id: String, jid: String, id: String) -> CommandResult<()> {
    let service = state.account_service(&account_id)?;
    let result = service.channel_revoke_post(&jid, &id).await;
    current(&state, &account_id, &service)?;
    result.map_err(|error| { service.note_error(&error); error.into() })
}

#[cfg(test)]
mod publishing_tests {
    use super::*;

    #[test]
    fn inline_channel_uploads_validate_base64_and_reject_large_payloads() {
        assert_eq!(inline_media("c3ludGhldGlj").unwrap(), b"synthetic");
        assert!(inline_media("not base64!").is_err());
        let oversized = "A".repeat(((1024 * 1024 + 2) / 3) * 4 + 4);
        assert!(inline_media(&oversized).is_err());
        assert_eq!(inline_media(&BASE64.encode(vec![0; 1024 * 1024])).unwrap().len(), 1024 * 1024);
        assert!(inline_media(&BASE64.encode(vec![0; 1024 * 1024 + 1])).is_err());
    }
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
