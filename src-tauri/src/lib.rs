use std::sync::Mutex;
use tauri::async_runtime::spawn;
use tauri::{AppHandle, Manager, State};
use tokio::time::{sleep, Duration};

/// Tracks completion of setup tasks before closing splashscreen
struct SetupState {
    frontend_task: bool,
    backend_task: bool,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
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
    // Simulate backend initialization
    // Real tasks (data dir creation, keyring init) will be added in Tasks 6-7
    sleep(Duration::from_millis(500)).await;

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
        .invoke_handler(tauri::generate_handler![greet, set_complete])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
