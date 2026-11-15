use futures_util::StreamExt;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use uuid::Uuid;

const CHUNK_SIZE: u64 = 2 * 1024 * 1024; // 2 MB chunks (multiples of 256KB for Drive API)

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TransferType {
    Download,
    Upload,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TransferStatus {
    Pending,
    Active,
    Paused,
    Completed,
    Error,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransferTask {
    pub id: String,
    pub transfer_type: TransferType,
    pub file_name: String,
    pub file_id: Option<String>,
    pub local_path: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub status: TransferStatus,
    pub session_uri: Option<String>,
    pub parent_id: Option<String>,
    pub speed_bytes_per_sec: u64,
    pub error_message: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct TransferProgressEvent {
    pub id: String,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub status: TransferStatus,
    pub error_message: Option<String>,
}

pub struct TransferManager {
    tasks: Arc<Mutex<Vec<TransferTask>>>,
    active_tokens: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>, // task_id -> is_cancelled/paused flag
    config_dir: PathBuf,
}

impl TransferManager {
    pub fn new() -> Self {
        let mut config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        config_dir.push("ggdown");
        let _ = fs::create_dir_all(&config_dir);

        let transfers_file = config_dir.join("transfers.json");
        let mut initial_tasks: Vec<TransferTask> = if transfers_file.exists() {
            fs::read_to_string(&transfers_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Reset active/pending tasks from previous sessions to paused
        for task in &mut initial_tasks {
            if task.status == TransferStatus::Active || task.status == TransferStatus::Pending {
                task.status = TransferStatus::Paused;
            }
        }

        Self {
            tasks: Arc::new(Mutex::new(initial_tasks)),
            active_tokens: Arc::new(Mutex::new(HashMap::new())),
            config_dir,
        }
    }

    async fn persist_tasks(&self) {
        let tasks = self.tasks.lock().await;
        let file = self.config_dir.join("transfers.json");
        if let Ok(json) = serde_json::to_string_pretty(&*tasks) {
            let _ = fs::write(file, json);
        }
    }

    pub async fn get_tasks(&self) -> Vec<TransferTask> {
        self.tasks.lock().await.clone()
    }

    pub async fn add_download(
        &self,
        file_id: String,
        file_name: String,
        file_size: u64,
        destination_dir: String,
        account_id: Option<String>,
    ) -> Result<TransferTask, String> {
        let local_path = PathBuf::from(&destination_dir).join(&file_name).to_string_lossy().to_string();
        let task = TransferTask {
            id: Uuid::new_v4().to_string(),
            transfer_type: TransferType::Download,
            file_name,
            file_id: Some(file_id),
            local_path,
            total_bytes: file_size,
            transferred_bytes: 0,
            status: TransferStatus::Pending,
            session_uri: None,
            parent_id: None,
            speed_bytes_per_sec: 0,
            error_message: None,
            account_id,
        };

        self.tasks.lock().await.push(task.clone());
        self.persist_tasks().await;
        Ok(task)
    }

    pub async fn add_upload(
        &self,
        file_path: String,
        parent_id: Option<String>,
        account_id: Option<String>,
    ) -> Result<TransferTask, String> {
        let path = PathBuf::from(&file_path);
        if !path.exists() {
            return Err("File does not exist".to_string());
        }

        let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
        let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let total_bytes = metadata.len();

        let task = TransferTask {
            id: Uuid::new_v4().to_string(),
            transfer_type: TransferType::Upload,
            file_name,
            file_id: None,
            local_path: file_path,
            total_bytes,
            transferred_bytes: 0,
            status: TransferStatus::Pending,
            session_uri: None,
            parent_id,
            speed_bytes_per_sec: 0,
            error_message: None,
            account_id,
        };

        self.tasks.lock().await.push(task.clone());
        self.persist_tasks().await;
        Ok(task)
    }

    pub async fn pause_task(&self, task_id: &str, app: &AppHandle) -> Result<(), String> {
        if let Some(stop_flag) = self.active_tokens.lock().await.get(task_id) {
            stop_flag.store(true, Ordering::SeqCst);
        }

        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = TransferStatus::Paused;
            task.speed_bytes_per_sec = 0;
            let _ = app.emit("transfer-progress", TransferProgressEvent {
                id: task.id.clone(),
                transferred_bytes: task.transferred_bytes,
                total_bytes: task.total_bytes,
                speed_bytes_per_sec: 0,
                status: TransferStatus::Paused,
                error_message: None,
            });
        }
        drop(tasks);
        self.persist_tasks().await;
        Ok(())
    }

    pub async fn cancel_task(&self, task_id: &str, app: &AppHandle) -> Result<(), String> {
        if let Some(stop_flag) = self.active_tokens.lock().await.get(task_id) {
            stop_flag.store(true, Ordering::SeqCst);
        }

        let mut tasks = self.tasks.lock().await;
        let mut part_to_remove = None;
        if let Some(pos) = tasks.iter().position(|t| t.id == task_id) {
            let task = tasks.remove(pos);
            if task.transfer_type == TransferType::Download {
                part_to_remove = Some(format!("{}.part", task.local_path));
            }
        }
        drop(tasks);
        self.persist_tasks().await;

        if let Some(part) = part_to_remove {
            let _ = fs::remove_file(part);
        }

        let _ = app.emit("transfer-removed", task_id);
        Ok(())
    }

    pub async fn resume_download(
        self: Arc<Self>,
        task_id: String,
        token: String,
        app: AppHandle,
    ) -> Result<(), String> {
        let (task, stop_flag) = {
            let mut tasks = self.tasks.lock().await;
            let task = tasks.iter_mut().find(|t| t.id == task_id).ok_or("Task not found")?;
            task.status = TransferStatus::Active;
            task.error_message = None;

            let stop_flag = Arc::new(AtomicBool::new(false));
            self.active_tokens.lock().await.insert(task_id.clone(), Arc::clone(&stop_flag));
            (task.clone(), stop_flag)
        };

        let _ = app.emit("transfer-progress", TransferProgressEvent {
            id: task.id.clone(),
            transferred_bytes: task.transferred_bytes,
            total_bytes: task.total_bytes,
            speed_bytes_per_sec: 0,
            status: TransferStatus::Active,
            error_message: None,
        });

        let this = Arc::clone(&self);
        tokio::spawn(async move {
            let part_path = format!("{}.part", task.local_path);
            let mut current_offset: u64 = 0;

            if let Ok(meta) = fs::metadata(&part_path) {
                current_offset = meta.len();
            }

            if current_offset >= task.total_bytes && task.total_bytes > 0 {
                // Download is already completed, finalize file
                let _ = fs::rename(&part_path, &task.local_path);
                this.mark_completed(&task.id, &app).await;
                return;
            }

            let file_id = match task.file_id.as_ref() {
                Some(id) => id.clone(),
                None => {
                    this.mark_error(&task.id, "Missing Google Drive file ID".to_string(), &app).await;
                    return;
                }
            };

            let client = reqwest::Client::new();
            let mut req = client
                .get(format!("https://www.googleapis.com/drive/v3/files/{}?alt=media", file_id))
                .header(AUTHORIZATION, format!("Bearer {}", token));

            if current_offset > 0 {
                req = req.header(reqwest::header::RANGE, format!("bytes={}-", current_offset));
            }

            let res = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    this.mark_error(&task.id, format!("Connection error: {}", e), &app).await;
                    return;
                }
            };

            if !res.status().is_success() && res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                this.mark_error(&task.id, format!("HTTP Error: {}", res.status()), &app).await;
                return;
            }

            let mut file = match OpenOptions::new().create(true).append(true).open(&part_path) {
                Ok(f) => f,
                Err(e) => {
                    this.mark_error(&task.id, format!("Failed to open destination: {}", e), &app).await;
                    return;
                }
            };

            let mut stream = res.bytes_stream();
            let mut downloaded = current_offset;
            let mut last_instant = Instant::now();
            let mut bytes_since_last_tick: u64 = 0;

            while let Some(chunk_result) = stream.next().await {
                if stop_flag.load(Ordering::SeqCst) {
                    // Task was paused or cancelled
                    return;
                }

                match chunk_result {
                    Ok(chunk) => {
                        let chunk_len = chunk.len() as u64;
                        if let Err(e) = file.write_all(&chunk) {
                            this.mark_error(&task.id, format!("Disk write error: {}", e), &app).await;
                            return;
                        }

                        downloaded += chunk_len;
                        bytes_since_last_tick += chunk_len;

                        let elapsed = last_instant.elapsed().as_millis();
                        if elapsed >= 500 {
                            let current_speed = (bytes_since_last_tick * 1000) / (elapsed as u64);
                            last_instant = Instant::now();
                            bytes_since_last_tick = 0;

                            this.update_progress(&task.id, downloaded, current_speed, &app).await;
                        }
                    }
                    Err(e) => {
                        // Connection dropped or interrupted
                        this.mark_error(&task.id, format!("Network error: {}", e), &app).await;
                        return;
                    }
                }
            }

            let _ = file.flush();
            drop(file);

            // Download completed, rename .part to original filename
            if let Err(e) = fs::rename(&part_path, &task.local_path) {
                this.mark_error(&task.id, format!("Rename error: {}", e), &app).await;
                return;
            }

            this.mark_completed(&task.id, &app).await;
        });

        Ok(())
    }

    pub async fn resume_upload(
        self: Arc<Self>,
        task_id: String,
        token: String,
        app: AppHandle,
    ) -> Result<(), String> {
        let (task, stop_flag) = {
            let mut tasks = self.tasks.lock().await;
            let task = tasks.iter_mut().find(|t| t.id == task_id).ok_or("Task not found")?;
            task.status = TransferStatus::Active;
            task.error_message = None;

            let stop_flag = Arc::new(AtomicBool::new(false));
            self.active_tokens.lock().await.insert(task_id.clone(), Arc::clone(&stop_flag));
            (task.clone(), stop_flag)
        };

        let _ = app.emit("transfer-progress", TransferProgressEvent {
            id: task.id.clone(),
            transferred_bytes: task.transferred_bytes,
            total_bytes: task.total_bytes,
            speed_bytes_per_sec: 0,
            status: TransferStatus::Active,
            error_message: None,
        });

        let this = Arc::clone(&self);
        tokio::spawn(async move {
            let client = reqwest::Client::new();

            // Step 1: Initiate session if not exists
            let session_uri = match task.session_uri {
                Some(ref uri) => uri.clone(),
                None => {
                    let mut init_headers = HeaderMap::new();
                    init_headers.insert(
                        AUTHORIZATION,
                        HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
                    );
                    init_headers.insert(
                        HeaderName::from_static("x-upload-content-type"),
                        HeaderValue::from_static("application/octet-stream"),
                    );
                    init_headers.insert(
                        HeaderName::from_static("x-upload-content-length"),
                        HeaderValue::from_str(&task.total_bytes.to_string()).unwrap(),
                    );
                    init_headers.insert(
                        CONTENT_TYPE,
                        HeaderValue::from_static("application/json; charset=UTF-8"),
                    );

                    let mut meta = serde_json::json!({ "name": task.file_name });
                    if let Some(ref pid) = task.parent_id {
                        meta["parents"] = serde_json::json!([pid]);
                    }

                    let init_res = match client
                        .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable")
                        .headers(init_headers)
                        .json(&meta)
                        .send()
                        .await
                    {
                        Ok(res) => res,
                        Err(e) => {
                            this.mark_error(&task.id, format!("Failed to initiate upload: {}", e), &app).await;
                            return;
                        }
                    };

                    if !init_res.status().is_success() {
                        this.mark_error(&task.id, format!("Initiate error: {}", init_res.status()), &app).await;
                        return;
                    }

                    match init_res.headers().get("location") {
                        Some(loc) => {
                            let uri = loc.to_str().unwrap_or_default().to_string();
                            this.save_session_uri(&task.id, &uri).await;
                            uri
                        }
                        None => {
                            this.mark_error(&task.id, "Missing upload session URI from Google".to_string(), &app).await;
                            return;
                        }
                    }
                }
            };

            // Step 2: Query upload status to know where to resume from
            let mut start_byte: u64 = 0;
            let status_res = client
                .put(&session_uri)
                .header(CONTENT_RANGE, format!("bytes */{}", task.total_bytes))
                .header(CONTENT_LENGTH, "0")
                .send()
                .await;

            if let Ok(res) = status_res {
                if res.status() == reqwest::StatusCode::PERMANENT_REDIRECT { // 308 Resume Incomplete
                    if let Some(range_header) = res.headers().get("range") {
                        if let Ok(range_str) = range_header.to_str() {
                            // Format: "bytes=0-123456"
                            if let Some(dash_idx) = range_str.find('-') {
                                if let Ok(last_byte) = range_str[dash_idx + 1..].parse::<u64>() {
                                    start_byte = last_byte + 1;
                                }
                            }
                        }
                    }
                } else if res.status().is_success() {
                    // Upload was already completed!
                    this.mark_completed(&task.id, &app).await;
                    return;
                }
            }

            if start_byte > 0 {
                this.update_progress(&task.id, start_byte, 0, &app).await;
            }

            // Step 3: Stream upload in chunks
            let mut file = match fs::File::open(&task.local_path) {
                Ok(f) => f,
                Err(e) => {
                    this.mark_error(&task.id, format!("Failed to open file: {}", e), &app).await;
                    return;
                }
            };

            let mut uploaded = start_byte;
            let mut last_instant = Instant::now();

            while uploaded < task.total_bytes {
                if stop_flag.load(Ordering::SeqCst) {
                    return;
                }

                let end_byte = std::cmp::min(uploaded + CHUNK_SIZE - 1, task.total_bytes - 1);
                let chunk_len = (end_byte - uploaded + 1) as usize;

                if let Err(e) = file.seek(SeekFrom::Start(uploaded)) {
                    this.mark_error(&task.id, format!("File seek error: {}", e), &app).await;
                    return;
                }

                let mut chunk_buffer = vec![0u8; chunk_len];
                if let Err(e) = file.read_exact(&mut chunk_buffer) {
                    this.mark_error(&task.id, format!("File read error: {}", e), &app).await;
                    return;
                }

                let range_header = format!("bytes {}-{}/{}", uploaded, end_byte, task.total_bytes);

                let mut retries = 0;
                let mut success = false;

                while retries < 3 && !success {
                    if stop_flag.load(Ordering::SeqCst) {
                        return;
                    }

                    let upload_chunk_res = client
                        .put(&session_uri)
                        .header(CONTENT_RANGE, &range_header)
                        .header(CONTENT_LENGTH, chunk_len.to_string())
                        .body(chunk_buffer.clone())
                        .send()
                        .await;

                    match upload_chunk_res {
                        Ok(res) => {
                            let status = res.status();
                            if status == reqwest::StatusCode::PERMANENT_REDIRECT || status.is_success() {
                                let elapsed = last_instant.elapsed().as_millis().max(1) as u64;
                                let speed = ((chunk_len as u64) * 1000) / elapsed;
                                last_instant = Instant::now();

                                uploaded = end_byte + 1;
                                this.update_progress(&task.id, uploaded, speed, &app).await;
                                success = true;

                                if status.is_success() {
                                    this.mark_completed(&task.id, &app).await;
                                    return;
                                }
                            } else {
                                retries += 1;
                                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                            }
                        }
                        Err(_) => {
                            retries += 1;
                            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        }
                    }
                }

                if !success {
                    this.mark_error(&task.id, "Upload connection interrupted. Click Resume to continue.".to_string(), &app).await;
                    return;
                }
            }

            this.mark_completed(&task.id, &app).await;
        });

        Ok(())
    }

    async fn save_session_uri(&self, task_id: &str, uri: &str) {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.session_uri = Some(uri.to_string());
        }
        drop(tasks);
        self.persist_tasks().await;
    }

    async fn update_progress(&self, task_id: &str, transferred: u64, speed: u64, app: &AppHandle) {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.transferred_bytes = transferred;
            task.speed_bytes_per_sec = speed;

            let _ = app.emit("transfer-progress", TransferProgressEvent {
                id: task.id.clone(),
                transferred_bytes: transferred,
                total_bytes: task.total_bytes,
                speed_bytes_per_sec: speed,
                status: TransferStatus::Active,
                error_message: None,
            });
        }
    }

    async fn mark_completed(&self, task_id: &str, app: &AppHandle) {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = TransferStatus::Completed;
            task.transferred_bytes = task.total_bytes;
            task.speed_bytes_per_sec = 0;
            task.error_message = None;

            let _ = app.emit("transfer-progress", TransferProgressEvent {
                id: task.id.clone(),
                transferred_bytes: task.total_bytes,
                total_bytes: task.total_bytes,
                speed_bytes_per_sec: 0,
                status: TransferStatus::Completed,
                error_message: None,
            });
        }
        drop(tasks);
        self.persist_tasks().await;
    }

    async fn mark_error(&self, task_id: &str, err: String, app: &AppHandle) {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = TransferStatus::Error;
            task.speed_bytes_per_sec = 0;
            task.error_message = Some(err.clone());

            let _ = app.emit("transfer-progress", TransferProgressEvent {
                id: task.id.clone(),
                transferred_bytes: task.transferred_bytes,
                total_bytes: task.total_bytes,
                speed_bytes_per_sec: 0,
                status: TransferStatus::Error,
                error_message: Some(err),
            });
        }
        drop(tasks);
        self.persist_tasks().await;
    }
}

