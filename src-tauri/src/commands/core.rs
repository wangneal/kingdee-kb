use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app_state::AppState;

const KEYRING_SERVICE: &str = "com.neal.kingdee-kb";

/// 跟踪启动任务完成状态，用于关闭启动画面
pub struct SetupState {
    pub frontend_task: bool,
    pub backend_task: bool,
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 确保 ~/.kingdee-kb/ 数据目录结构存在
pub fn ensure_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let data_dir = home.join(".kingdee-kb");

    let subdirs = ["knowledge", "index", "models", "bm25_index", "products"];
    for subdir in subdirs {
        fs::create_dir_all(data_dir.join(subdir))
            .map_err(|e| format!("Failed to create {}: {}", subdir, e))?;
    }

    Ok(data_dir)
}

/// 递归复制目录及其所有内容
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("Failed to create dir {}: {}", dst.display(), e))?;
    for entry in fs::read_dir(src).map_err(|e| format!("Failed to read dir {}: {}", src.display(), e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {} to {}: {}", src_path.display(), dst_path.display(), e))?;
        }
    }
    Ok(())
}

/// 获取数据目录路径（供前端使用）
#[tauri::command]
pub fn get_data_dir() -> Result<String, String> {
    let data_dir = ensure_data_dir()?;
    Ok(data_dir.to_string_lossy().to_string())
}

/// 存储 API 密钥到系统凭据存储
#[tauri::command]
pub fn set_api_key(service: String, key: String) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to create keyring entry: {}", e))?;
    entry
        .set_password(&key)
        .map_err(|e| format!("Failed to store API key: {}", e))?;
    Ok(())
}

/// 从系统凭据存储获取 API 密钥
#[tauri::command]
pub fn get_api_key(service: String) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to retrieve API key: {}", e)),
    }
}

/// 从系统凭据存储删除 API 密钥
#[tauri::command]
pub fn delete_api_key(service: String) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &service)
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    entry
        .delete_credential()
        .map_err(|e| format!("Failed to delete API key: {}", e))?;
    Ok(())
}

/// 前端 React 挂载完成后的回调
#[tauri::command]
pub async fn set_complete(
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

    if state_lock.frontend_task && state_lock.backend_task {
        if let Some(splash_window) = app.get_webview_window("splashscreen") {
            let _ = splash_window.close();
        }
        if let Some(main_window) = app.get_webview_window("main") {
            let _ = main_window.show();
            let _ = main_window.set_focus();
        }
    }

    Ok(())
}

/// 使用 PowerShell 将内容写入文件（UTF-8 BOM 编码）
fn write_file_via_powershell(path: &Path, content: &str) -> Result<(), String> {
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, content)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    let ps_script = format!(
        "$c = Get-Content -Path '{}' -Raw -Encoding UTF8; [System.IO.File]::WriteAllText('{}', $c, [System.Text.UTF8Encoding]::new($true))",
        temp_path.to_string_lossy().replace("'", "''"),
        path.to_string_lossy().replace("'", "''")
    );

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
        .output()
        .map_err(|e| format!("PowerShell failed: {}", e))?;

    let _ = std::fs::remove_file(&temp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PowerShell write error: {}", stderr));
    }

    Ok(())
}

/// 将任意内容导出到文件（UTF-8 BOM 编码）
#[tauri::command]
pub async fn export_report(
    content: String,
    file_path: String,
) -> Result<String, String> {
    let path = PathBuf::from(&file_path);
    write_file_via_powershell(&path, &content)?;
    Ok(file_path)
}

/// 执行后端初始化任务（异步，不阻塞 UI 启动）
pub async fn setup_backend(app: AppHandle) -> Result<(), String> {
    let data_dir = ensure_data_dir()?;
    println!("Data directory initialized at: {:?}", data_dir);

    // 初始化阶段 2 服务
    match AppState::new(&data_dir) {
        Ok(app_state) => {
            app.manage(app_state);
            println!("Phase 2 services initialized (embedding, vector index, metadata)");
        }
        Err(e) => {
            eprintln!("WARNING: Phase 2 services failed to initialize: {}", e);
            eprintln!("The app will start in limited mode (no embedding/search/LLM).");
            app.manage(AppState::minimal(&data_dir));
        }
    }

    // 异步自动加载缓存的嵌入模型（后台加载，不阻塞前端启动）
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_clone.state::<AppState>();
        let mm_arc = state.model_manager.clone();
        let emb_arc = state.embedding.clone();
        drop(state);

        let model = {
            let mut mm = match mm_arc.lock() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Auto-load: model_manager lock error: {}", e);
                    return;
                }
            };
            if let Err(e) = mm.init() {
                eprintln!("Auto-load embedding model init failed: {}", e);
                return;
            }
            match mm.take_model() {
                Some(m) => m,
                None => {
                    eprintln!("Auto-load: model initialized but take_model returned None");
                    return;
                }
            }
        };

        {
            let mut emb = match emb_arc.lock() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Auto-load: embedding lock error: {}", e);
                    return;
                }
            };
            emb.set_model(model);
            println!("Embedding model auto-loaded from local cache!");
            let _ = app_clone.emit("model-ready", ());
        }
    });

    // 确保模板目录存在，如果为空则同步内置模板
    let template_dir = data_dir.join("templates");
    if !template_dir.exists() {
        std::fs::create_dir_all(&template_dir)
            .map_err(|e| format!("Failed to create templates directory: {}", e))?;
        println!("Created templates directory at: {:?}", template_dir);
    }

    // 如果模板目录为空，从应用包中复制内置模板
    if std::fs::read_dir(&template_dir)
        .map_err(|e| format!("Failed to read templates directory: {}", e))?
        .next()
        .is_none()
    {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let resource_dir = exe_dir.join("templates");
                if resource_dir.exists() {
                    match copy_dir_recursive(&resource_dir, &template_dir) {
                        Ok(_) => println!("Copied built-in templates to {:?}", template_dir),
                        Err(e) => eprintln!("Warning: Failed to copy built-in templates: {}", e),
                    }
                }
            }
        }
        let dev_template_dir = std::path::PathBuf::from("../templates");
        if template_dir
            .read_dir()
            .map_err(|e| format!("Failed to read templates directory: {}", e))?
            .next()
            .is_none()
            && dev_template_dir.exists()
        {
            match copy_dir_recursive(&dev_template_dir, &template_dir) {
                Ok(_) => println!("Copied dev templates to {:?}", template_dir),
                Err(e) => eprintln!("Warning: Failed to copy dev templates: {}", e),
            }
        }
    }

    let _ = set_complete(
        app.clone(),
        app.state::<Mutex<SetupState>>(),
        "backend".to_string(),
    )
    .await;

    Ok(())
}
