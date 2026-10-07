use std::sync::Arc;
use postal_core::service::CallRecord;
use tauri::State;
use crate::{AppState, command_error::{CommandError, CommandResult}};

#[tauri::command]
pub(crate) async fn call_history(state: State<'_, AppState>, account: String, limit: Option<u32>) -> CommandResult<Vec<CallRecord>> {
    let service = state.account_service(&account)?;
    let calls = service.call_history(limit.unwrap_or(200)).await.map_err(CommandError::from)?;
    let current = state.account_service(&account)?;
    if !Arc::ptr_eq(&service, &current) { return Err(CommandError::code("error.account_changed")); }
    Ok(calls)
}
