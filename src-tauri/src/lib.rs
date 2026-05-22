use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::async_runtime::spawn;
use tauri::{AppHandle, Manager, State};

/// Tracks completion of setup tasks before closing splashscreen
struct SetupState {
    frontend_task: bool,
    backend_task: bool,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// Ensure the ~/.kingdee-kb/ data directory structure exists
fn ensure_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let data_dir = home.join(".kingdee-kb");

    let subdirs = ["knowledge", "index", "models", "bm25_index"];
    for subdir in subdirs {
        fs::create_dir_all(data_dir.join(subdir))
            .map_err(|e| format!("Failed to create {}: {}", subdir, e))?;
    }

    // Create metadata.db empty file (SQLite will initialize it later)
    let db_path = data_dir.join("metadata.db");
    if !db_path.exists() {
        fs::File::create(&db_path)
            .map_err(|e| format!("Failed to create metadata.db: {}", e))?;
    }

    Ok(data_dir)
}

/// Get the data directory path (available to frontend)
#[tauri::command]
fn get_data_dir() -> Result<String, String> {
    let data_dir = ensure_data_dir()?;
    Ok(data_dir.to_string_lossy().to_string())
}

/// Called by the frontend when React has mounted and is ready
#[tauri::command]
async fn set_complete(
    app: AppHandle,
    state: State<'_, Mutex<SetupState>>,
    task: String,
) -> Result<(), String> {
    let mut state_lock = state.lock().map_err(|e| e.to_string())?;
    match task.as_str() {
        "frontend" => state_lock.frontend_task = true,
        "backend" => state_lock.backend_task = true,
        _ => return Err(format!("invalid task: {}", task)),
    }

    // Close splashscreen and show main window when both tasks are complete
    if state_lock.frontend_task && state_lock.backend_task {
        if let Some(splash_window) = app.get_webview_window("splashscreen") {
            let _ = splash_window.close();
        }
        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.show();
        }
    }

    Ok(())
}

/// Perform backend initialization tasks
async fn setup_backend(app: AppHandle) -> Result<(), String> {
    // Create data directory structure on first launch
    let data_dir = ensure_data_dir()?;
    println!("Data directory initialized at: {:?}", data_dir);

    set_complete(
        app.clone(),
        app.state::<Mutex<SetupState>>(),
        "backend".to_string(),
    )
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(SetupState {
            frontend_task: false,
            backend_task: false,
        }))
        .setup(|app| {
            // Start backend setup asynchronously
            spawn(setup_backend(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet, set_complete, get_data_dir])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
