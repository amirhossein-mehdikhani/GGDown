mod auth;
mod drive;
mod transfer;

use auth::{AuthCredentials, AuthManager, TokenData};
use drive::{DriveFile, DriveManager};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, State};
use transfer::{TransferManager, TransferTask};

pub struct AppState {
    pub auth: Arc<AuthManager>,
    pub drive: Arc<DriveManager>,
    pub transfer: Arc<TransferManager>,
}

#[derive(Serialize)]
pub struct AuthStatusResponse {
    pub is_logged_in: bool,
    pub has_credentials: bool,
    pub client_id: Option<String>,
    pub user: Option<TokenData>,
}

#[tauri::command]
async fn auth_get_status(state: State<'_, AppState>) -> Result<AuthStatusResponse, String> {
    let creds_lock = state.auth.credentials.lock().await;
    let token_lock = state.auth.token_data.lock().await;

    let has_credentials = creds_lock.is_some();
    let client_id = creds_lock.as_ref().map(|c| c.client_id.clone());
    let is_logged_in = token_lock.is_some();
    let user = token_lock.clone();

    Ok(AuthStatusResponse {
        is_logged_in,
        has_credentials,
        client_id,
        user,
    })
}

#[tauri::command]
async fn auth_save_credentials(
    client_id: String,
    client_secret: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .auth
        .save_credentials(AuthCredentials {
            client_id,
            client_secret,
        })
        .await
}

#[tauri::command]
async fn auth_start_login(state: State<'_, AppState>) -> Result<String, String> {
    let creds = {
        let creds_lock = state.auth.credentials.lock().await;
        creds_lock
            .as_ref()
            .ok_or_else(|| "Please enter and save your Client ID and Client Secret first.".to_string())?
            .clone()
    };

    state
        .auth
        .start_oauth_flow(creds.client_id, creds.client_secret)
        .await
}

#[tauri::command]
async fn auth_logout(state: State<'_, AppState>) -> Result<(), String> {
    state.auth.clear_session().await
}

#[tauri::command]
async fn drive_list(
    folder_id: Option<String>,
    search: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<DriveFile>, String> {
    let token = state.auth.get_valid_token().await?;
    state.drive.list_files(&token, folder_id, search).await
}

#[tauri::command]
async fn drive_create_folder(
    name: String,
    parent_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<DriveFile, String> {
    let token = state.auth.get_valid_token().await?;
    state.drive.create_folder(&token, &name, parent_id).await
}

#[tauri::command]
async fn drive_delete(file_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let token = state.auth.get_valid_token().await?;
    state.drive.delete_file(&token, &file_id).await
}

#[tauri::command]
async fn drive_rename(
    file_id: String,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let token = state.auth.get_valid_token().await?;
    state.drive.rename_file(&token, &file_id, &new_name).await
}

#[tauri::command]
async fn transfer_list(state: State<'_, AppState>) -> Result<Vec<TransferTask>, String> {
    Ok(state.transfer.get_tasks().await)
}

#[tauri::command]
async fn transfer_add_download(
    file_id: String,
    file_name: String,
    file_size: u64,
    destination_dir: Option<String>,
    state: State<'_, AppState>,
) -> Result<TransferTask, String> {
    let dest = destination_dir.unwrap_or_else(|| {
        dirs::download_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")))
            .to_string_lossy()
            .to_string()
    });

    state
        .transfer
        .add_download(file_id, file_name, file_size, dest)
        .await
}

#[tauri::command]
async fn transfer_add_upload(
    file_path: String,
    parent_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<TransferTask, String> {
    state.transfer.add_upload(file_path, parent_id).await
}

#[tauri::command]
async fn transfer_start(
    task_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let token = state.auth.get_valid_token().await?;
    let tasks = state.transfer.get_tasks().await;
    let task = tasks.iter().find(|t| t.id == task_id).ok_or("Task not found")?;

    match task.transfer_type {
        transfer::TransferType::Download => {
            state.transfer.clone().resume_download(task_id, token, app).await
        }
        transfer::TransferType::Upload => {
            state.transfer.clone().resume_upload(task_id, token, app).await
        }
    }
}

#[tauri::command]
async fn transfer_pause(
    task_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    state.transfer.pause_task(&task_id, &app).await
}

#[tauri::command]
async fn transfer_cancel(
    task_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    state.transfer.cancel_task(&task_id, &app).await
}

#[tauri::command]
fn open_browser_url(url: String) -> Result<(), String> {
    auth::open_in_browser(&url);
    Ok(())
}

#[tauri::command]
async fn system_pick_files() -> Result<Vec<String>, String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("zenity")
            .args(&["--file-selection", "--multiple", "--separator=|", "--title=Select files to upload"])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                let paths: Vec<String> = s
                    .trim()
                    .split('|')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                if !paths.is_empty() {
                    return Ok(paths);
                }
            }
        }
    }

    if let Some(files) = rfd::AsyncFileDialog::new()
        .set_title("Select files to upload")
        .pick_files()
        .await
    {
        let paths = files
            .into_iter()
            .map(|f| f.path().to_string_lossy().to_string())
            .collect();
        return Ok(paths);
    }

    Ok(Vec::new())
}

#[tauri::command]
fn system_get_default_download_dir() -> String {
    dirs::download_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")))
        .to_string_lossy()
        .to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState {
        auth: Arc::new(AuthManager::new()),
        drive: Arc::new(DriveManager::new()),
        transfer: Arc::new(TransferManager::new()),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            auth_get_status,
            auth_save_credentials,
            auth_start_login,
            auth_logout,
            drive_list,
            drive_create_folder,
            drive_delete,
            drive_rename,
            transfer_list,
            transfer_add_download,
            transfer_add_upload,
            transfer_start,
            transfer_pause,
            transfer_cancel,
            open_browser_url,
            system_pick_files,
            system_get_default_download_dir
        ])
        .run(tauri::generate_context!())
        .expect("error while running GGDown application");
}
