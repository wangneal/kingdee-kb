use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app_state::AppState;
use crate::services::skill_manager::SkillManager;

const KEYRING_SERVICE: &str = "com.neal.kingdee-kb";

/// 跟踪启动任务完成状态，用于关闭启动画面
pub struct SetupState {
    pub frontend_task: bool,
    pub backend_task: bool,
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
    std::fs::write(&temp_path, content).map_err(|e| format!("Failed to write temp file: {}", e))?;

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
pub async fn export_report(content: String, file_path: String) -> Result<String, String> {
    let path = PathBuf::from(&file_path);
    write_file_via_powershell(&path, &content)?;
    Ok(file_path)
}

/// 确保 ~/.kingdee-kb/ 数据目录结构存在
pub fn ensure_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let data_dir = home.join(".kingdee-kb");

    let subdirs = [
        "knowledge",
        "index",
        "models",
        "bm25_index",
        "products",
        "skills",
    ];
    for subdir in subdirs {
        fs::create_dir_all(data_dir.join(subdir))
            .map_err(|e| format!("Failed to create {}: {}", subdir, e))?;
    }

    Ok(data_dir)
}

/// 递归复制目录及其所有内容
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create dir {}: {}", dst.display(), e))?;
    for entry in
        fs::read_dir(src).map_err(|e| format!("Failed to read dir {}: {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|e| {
                format!(
                    "Failed to copy {} to {}: {}",
                    src_path.display(),
                    dst_path.display(),
                    e
                )
            })?;
        }
    }
    Ok(())
}

fn dir_has_entries(path: &Path) -> bool {
    path.read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn seed_skills_dir(_app: &AppHandle, data_dir: &Path) -> Result<PathBuf, String> {
    let skills_dir = data_dir.join("skills");
    fs::create_dir_all(&skills_dir).map_err(|e| {
        format!(
            "Failed to create skills dir {}: {}",
            skills_dir.display(),
            e
        )
    })?;

    Ok(skills_dir)
}

/// 初始化 AppState（同步，在 setup 中立刻调用）
pub fn init_app_state(app: &AppHandle) -> Result<AppState, String> {
    let data_dir = ensure_data_dir()?;
    println!("Data directory initialized at: {:?}", data_dir);

    // 初始化技能管理器。外部 skill 包先拷贝到用户数据目录，后续导入也写入同一位置。
    let skills_dir = seed_skills_dir(app, &data_dir)?;
    let skill_manager = SkillManager::new(skills_dir);
    println!(
        "Skill manager initialized with {} skills",
        skill_manager.count()
    );

    // 初始化阶段 2 服务
    match AppState::new(&data_dir, skill_manager) {
        Ok(app_state) => {
            println!("Phase 2 services initialized (embedding, vector index, metadata)");
            Ok(app_state)
        }
        Err(e) => {
            eprintln!("WARNING: Phase 2 services failed to initialize: {}", e);
            eprintln!("The app will start in limited mode (no embedding/search/LLM).");
            let app_state = AppState::minimal(&data_dir);
            Ok(app_state)
        }
    }
}

/// 执行后端初始化异步任务（在 AppState 被托管后运行）
pub async fn setup_backend_async(app: AppHandle) -> Result<(), String> {
    let app_state = app.state::<AppState>();
    let data_dir = &app_state.data_dir;

    // 后台异步复制内置技能（首次运行），防止阻塞 UI 主线程
    let skills_dir = data_dir.join("skills");
    if !dir_has_entries(&skills_dir) {
        let mut candidates = Vec::new();
        if let Ok(resource_dir) = app.path().resource_dir() {
            candidates.push(resource_dir.join("skills"));
        }
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                candidates.push(exe_dir.join("skills"));
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("skills"));
            candidates.push(cwd.join("..").join("skills"));
        }

        if let Some(source) = candidates.into_iter().find(|path| path.exists()) {
            match copy_dir_recursive(&source, &skills_dir) {
                Ok(_) => {
                    println!(
                        "Copied built-in skills in background from {:?} to {:?}",
                        source, &skills_dir
                    );
                    let mut sm = app_state.skill_manager.lock().await;
                    sm.scan();
                    println!(
                        "Skill manager background scan complete. Loaded {} skills",
                        sm.count()
                    );
                }
                Err(e) => eprintln!(
                    "Warning: failed to seed built-in skills in background: {}",
                    e
                ),
            }
        }
    }

    // 启动时补偿 pending 删除
    crate::compensate_pending_deletions(&app_state);

    // 启动时恢复备份/摄入队列中的任务（如果有）
    if let Err(e) = crate::commands::ingestion_queue::process_pending_queue(&app_state) {
        tracing::warn!("恢复摄入队列失败: {}", e);
    }

    // 嵌入模型改为"首次使用时懒加载"，不占用启动时间。
    println!("Embedding model will be loaded on first use (lazy load).");

    // 无论后台异步同步任务是否发生非致命错误，都必须调用 set_complete，以防前端白屏挂死
    let _ = set_complete(
        app.clone(),
        app.state::<Mutex<SetupState>>(),
        "backend".to_string(),
    )
    .await;

    // 异步预加载 Embedding 模型，避免首次检索时因加载模型导致卡顿
    // 注意：Reranker 模型 (bge-reranker-v2-m3) 占 2.1GB 内存，改为首次搜索时懒加载
    let app_clone = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(state) = app_clone.try_state::<AppState>() {
            println!("后台异步预加载 Embedding 模型中...");
            state.ensure_embedding_ready();
        }
    });

    // 后台定时检查：空闲超过 5 分钟自动释放本地 Embedding 模型（~90MB）
    // 下次使用时 ensure_embedding_ready() 会从磁盘缓存重新加载（毫秒级）
    let app_for_idle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        const IDLE_TIMEOUT_SECS: u64 = 300; // 5 分钟
        const CHECK_INTERVAL_SECS: u64 = 60; // 每 60 秒检查一次
        loop {
            std::thread::sleep(std::time::Duration::from_secs(CHECK_INTERVAL_SECS));
            if let Some(state) = app_for_idle.try_state::<AppState>() {
                if state.unload_idle_embedding(IDLE_TIMEOUT_SECS) {
                    println!(
                        "后台检查：本地 Embedding 模型空闲超过 {}秒，已自动释放内存",
                        IDLE_TIMEOUT_SECS
                    );
                }
            }
        }
    });

    // 后台定时熵管理：每小时扫描过期技能/索引漂移，发现异常时通知前端
    // 符合 Harness Engineering 的“垃圾回收”理念：持续小额偿还技术债
    let app_for_entropy = app.clone();
    let data_dir_for_entropy = app
        .try_state::<AppState>()
        .map(|s| s.data_dir.clone())
        .unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        use crate::services::harness::entropy::EntropyManager;
        const SCAN_INTERVAL_SECS: u64 = 3600; // 每小时扫描一次
        loop {
            std::thread::sleep(std::time::Duration::from_secs(SCAN_INTERVAL_SECS));
            if data_dir_for_entropy.as_os_str().is_empty() {
                continue;
            }

            let mgr = EntropyManager::new(data_dir_for_entropy.clone());

            // 扫描过期技能
            let stale_skills = mgr.scan_stale_files("skills");
            if !stale_skills.is_empty() {
                let items: Vec<serde_json::Value> = stale_skills
                    .iter()
                    .map(|item| {
                        serde_json::json!({
                            "path": item.path.display().to_string(),
                            "days": item.last_accessed_days,
                            "type": "skill"
                        })
                    })
                    .collect();
                let _ = app_for_entropy.emit(
                    "entropy-warning",
                    serde_json::json!({
                        "kind": "stale-skills",
                        "count": items.len(),
                        "items": items
                    }),
                );
                println!("后台熵检查：发现 {} 个过期技能", stale_skills.len());
            }

            // 扫描索引漂移
            let drifts = mgr.scan_index_drift("sources", "index");
            if !drifts.is_empty() {
                let _ = app_for_entropy.emit(
                    "entropy-warning",
                    serde_json::json!({
                        "kind": "index-drift",
                        "count": drifts.len()
                    }),
                );
                println!("后台熵检查：发现 {} 个索引漂移", drifts.len());
            }
        }
    });

    Ok(())
}

// ── Entropy Management Commands ──────────────────────────────────────────

/// 另存文件附件（Rust 层流式拷贝，零前端内存占用）
#[tauri::command]
pub async fn save_attachment_as(source: String, dest: String) -> Result<String, String> {
    let src_path = PathBuf::from(&source);
    let dst_path = PathBuf::from(&dest);

    // 确保目标目录存在
    if let Some(parent) = dst_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建目标目录: {}", e))?;
    }

    // 流式拷贝
    fs::copy(&src_path, &dst_path).map_err(|e| format!("文件拷贝失败: {}", e))?;

    Ok(dest)
}
