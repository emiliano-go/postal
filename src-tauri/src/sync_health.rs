use crate::{
    command_error::{CommandError, CommandResult},
    AppState,
};
use postal_core::service::{SyncCollection, SyncHealthView, SyncMode, SyncRepairReport};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub(crate) async fn sync_health(
    state: State<'_, AppState>,
    account_id: String,
) -> CommandResult<SyncHealthView> {
    let service = state
        .account_service(&account_id)
        .map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service.sync_health().await.map_err(CommandError::from)?;
    let current = state
        .account_service(&account_id)
        .map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(result)
}

#[tauri::command]
pub(crate) async fn repair_sync(
    state: State<'_, AppState>,
    account_id: String,
    collections: Option<Vec<SyncCollection>>,
    mode: SyncMode,
) -> CommandResult<SyncRepairReport> {
    let service = state
        .account_service(&account_id)
        .map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    let result = service
        .repair_sync(collections, mode)
        .await
        .map_err(|error| {
            service.note_error(&error);
            CommandError::from(error)
        })?;
    let current = state
        .account_service(&account_id)
        .map_err(|error| CommandError::code("error.account_changed").with_diagnostic(error))?;
    if !Arc::ptr_eq(&service, &current) {
        return Err(CommandError::code("error.account_changed"));
    }
    Ok(result)
}
