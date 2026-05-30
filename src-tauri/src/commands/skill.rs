//! 技能系统 Tauri 命令
//!
//! 提供前端调用的技能管理接口：
//!   - list_skills / get_skill / search_skills

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

use crate::app_state::AppState;
use crate::services::skill_types::{Skill, SkillFull, SharedResource, SkillFile};
use crate::services::skill_trigger::{SkillMatch, TriggerContext};
use crate::services::prompt_assembler::SkillPromptEntry;
use crate::services::skill_executor::{ExecutionResult, SubstitutionContext};
use crate::services::signal_writer::SignalEvent;
use crate::services::template_manager::TemplateManifest;

/// 列出所有技能
#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<Skill>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.list_all())
}

/// 按名称获取技能详情
#[tauri::command]
pub async fn get_skill(state: State<'_, AppState>, name: String) -> Result<Option<Skill>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.get(&name))
}

/// 搜索技能
#[tauri::command]
pub async fn search_skills(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<Skill>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.search(&query))
}

/// 获取技能统计
#[tauri::command]
pub async fn get_skill_stats(state: State<'_, AppState>) -> Result<SkillStatsResponse, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    let by_category = manager.stats().into_iter().collect();
    Ok(SkillStatsResponse {
        total: manager.count(),
        by_category,
    })
}

/// 重新扫描技能目录
#[tauri::command]
pub async fn rescan_skills(state: State<'_, AppState>) -> Result<SkillScanResult, String> {
    let mut manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    manager.scan();
    Ok(SkillScanResult {
        total: manager.count(),
        by_category: manager.stats().into_iter().collect(),
    })
}

/// 匹配最佳技能
#[tauri::command]
pub async fn match_skill(
    state: State<'_, AppState>,
    input: String,
) -> Result<Option<Skill>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.match_best(&input))
}

/// 从一个 SKILL.md 文件导入新技能，复制到 skills/ 目录
#[tauri::command]
pub async fn import_skill(state: State<'_, AppState>, file_path: String) -> Result<String, String> {
    let mut manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    let src = PathBuf::from(&file_path);

    if !src.exists() {
        return Err(format!("文件不存在: {}", file_path));
    }
    if !src.is_file() {
        return Err("路径不是文件".to_string());
    }

    let content = std::fs::read_to_string(&src).map_err(|e| format!("读取文件失败: {}", e))?;

    // 解析技能名
    let (meta, _) = crate::services::skill_manager::SkillManager::parse_skill_md_public(&content);
    let name = import_skill_name(&src, meta.name.as_deref());

    // 复制到 skills/<name>/SKILL.md
    manager.import_skill(&name, &content)
}

/// 获取技能完整信息（含支撑文件和共享资源）
#[tauri::command]
pub async fn get_skill_full(state: State<'_, AppState>, name: String) -> Result<Option<SkillFull>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.get_skill_full(&name))
}

/// 获取所有共享资源
#[tauri::command]
pub async fn list_shared_resources(state: State<'_, AppState>) -> Result<Vec<SharedResource>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.get_shared_resources())
}

/// 读取技能支撑文件内容
#[tauri::command]
pub async fn read_skill_file(
    state: State<'_, AppState>,
    skill_name: String,
    relative_path: String,
) -> Result<String, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    manager.read_skill_file(&skill_name, &relative_path)
}

/// 获取技能支撑文件列表
#[tauri::command]
pub async fn list_skill_files(state: State<'_, AppState>, name: String) -> Result<Vec<SkillFile>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.get_skill_files(&name))
}

fn import_skill_name(src: &std::path::Path, metadata_name: Option<&str>) -> String {
    let metadata_name = metadata_name.unwrap_or("").trim();
    if is_safe_import_name(metadata_name) && metadata_name != "kingdee-implementation-suite" {
        return metadata_name.to_string();
    }

    src.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|name| is_safe_import_name(name))
        .or_else(|| {
            src.file_stem()
                .and_then(|n| n.to_str())
                .filter(|name| is_safe_import_name(name))
        })
        .unwrap_or("unknown")
        .to_string()
}

fn is_safe_import_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ─── Phase 2: 触发匹配命令 ──────────────────────────────────

/// 触发技能匹配（使用完整触发上下文）
#[tauri::command]
pub async fn trigger_skill_match(
    state: State<'_, AppState>,
    context: TriggerContext,
) -> Result<Vec<SkillMatch>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    let mut matches = Vec::new();

    // 使用触发引擎匹配
    if let Some(best) = manager.match_best_skill(&context) {
        matches.push(best);
    }

    // 补充：匹配多个候选
    let candidates = manager.match_candidates(&context.user_input, 5);
    for candidate in candidates {
        if !matches.iter().any(|m| m.skill_id == candidate.skill_id) {
            matches.push(candidate);
        }
    }

    // 记录匹配事件
    if let Some(first) = matches.first() {
        if let Ok(writer) = state.signal_writer.lock() {
            let match_type = match first.match_type {
                crate::services::skill_trigger::MatchType::Keyword => "keyword",
                crate::services::skill_trigger::MatchType::Semantic => "semantic",
                crate::services::skill_trigger::MatchType::Path => "path",
            };
            let event = SignalEvent::skill_matched(&first.skill_id, first.score, match_type);
            let _ = writer.write(event);
        }
    }

    Ok(matches)
}

/// 匹配多个候选技能
#[tauri::command]
pub async fn match_skill_candidates(
    state: State<'_, AppState>,
    input: String,
    limit: Option<usize>,
) -> Result<Vec<SkillMatch>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.match_candidates(&input, limit.unwrap_or(5)))
}

/// 生成技能列表系统提示
#[tauri::command]
pub async fn get_skill_list_prompt(state: State<'_, AppState>) -> Result<String, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.build_skill_list_prompt())
}

/// 获取技能摘要列表（用于前端展示和提示注入）
#[tauri::command]
pub async fn get_skill_prompt_entries(
    state: State<'_, AppState>,
) -> Result<Vec<SkillPromptEntry>, String> {
    let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
    Ok(manager.get_skill_prompt_entries())
}

// ─── Phase 3: 脚本执行命令 ──────────────────────────────────

/// 执行技能脚本
#[tauri::command]
pub async fn execute_skill_script(
    state: State<'_, AppState>,
    skill_id: String,
    script_path: String,
    arguments: Vec<String>,
) -> Result<ExecutionResult, String> {
    // 提取所需数据，避免在 await 时持有 MutexGuard
    let (skill_dir, skills_dir, script_content) = {
        let manager = state.skill_manager.lock().map_err(|e| e.to_string())?;
        let skill_dir = manager.get_skill_dir(&skill_id);
        let skills_dir = manager.get_skills_dir();
        let script_content = manager.read_skill_file(&skill_id, &script_path)?;
        (skill_dir, skills_dir, script_content)
    };

    // 创建替换上下文
    let context = SubstitutionContext {
        arguments,
        skill_dir,
        session_id: uuid::Uuid::new_v4().to_string(),
        custom_vars: std::collections::HashMap::new(),
    };

    // 创建执行器
    let config = crate::services::skill_executor::ExecutorConfig {
        allowed_skills: std::collections::HashSet::new(), // 允许所有技能
        timeout: 30,
        working_dir: skills_dir,
    };
    let executor = crate::services::skill_executor::SkillExecutor::new(config);

    // 检测脚本语言
    let lang = if script_path.ends_with(".py") {
        "python"
    } else if script_path.ends_with(".sh") {
        "bash"
    } else if script_path.ends_with(".ps1") {
        "powershell"
    } else {
        "bash"
    };

    // 执行脚本
    let result = executor
        .execute_block_command(lang, &script_content, &context)
        .await
        .map_err(|e| e.to_string())?;

    // 记录执行事件
    if let Ok(writer) = state.signal_writer.lock() {
        let event = SignalEvent::skill_executed(&skill_id, result.success, result.duration_ms);
        let _ = writer.write(event);
    }

    Ok(result)
}

// ─── Phase 3: 模板管理命令 ──────────────────────────────────

/// 获取模板清单
#[tauri::command]
pub async fn get_template_manifest(
    state: State<'_, AppState>,
) -> Result<Option<TemplateManifest>, String> {
    // 模板管理器的清单存储在应用数据目录
    let manifest_path = state.data_dir.join("template-manifest.json");
    if !manifest_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取模板清单失败: {}", e))?;

    let manifest: TemplateManifest =
        serde_json::from_str(&content).map_err(|e| format!("解析模板清单失败: {}", e))?;

    Ok(Some(manifest))
}

/// 保存模板清单
#[tauri::command]
pub async fn save_template_manifest(
    state: State<'_, AppState>,
    manifest: TemplateManifest,
) -> Result<(), String> {
    let manifest_path = state.data_dir.join("template-manifest.json");

    let content =
        serde_json::to_string_pretty(&manifest).map_err(|e| format!("序列化模板清单失败: {}", e))?;

    std::fs::write(&manifest_path, content)
        .map_err(|e| format!("写入模板清单失败: {}", e))?;

    Ok(())
}

// ─── Phase 4: 图像处理命令 ──────────────────────────────────

/// 检查图像处理依赖状态
#[tauri::command]
pub async fn check_image_deps(state: State<'_, AppState>) -> Result<ImageDepsStatus, String> {
    let processor = state.image_processor.lock().map_err(|e| e.to_string())?;
    
    Ok(ImageDepsStatus {
        ocr_configured: processor.has_ocr(),
        vision_configured: processor.has_vision(),
        ocr_provider: None, // TODO: 从配置中获取
        vision_provider: None, // TODO: 从配置中获取
    })
}

/// 保存图像处理 API 配置
#[tauri::command]
pub async fn save_image_config(
    state: State<'_, AppState>,
    ocr_provider: Option<String>,
    ocr_api_key: Option<String>,
    ocr_secret_key: Option<String>,
    vision_provider: Option<String>,
    vision_api_key: Option<String>,
    vision_base_url: Option<String>,
) -> Result<(), String> {
    let mut processor = state.image_processor.lock().map_err(|e| e.to_string())?;
    
    // 配置 OCR
    if let (Some(provider), Some(api_key)) = (ocr_provider, ocr_api_key) {
        let ocr_provider = match provider.as_str() {
            "baidu" => crate::services::image_processor::OcrProvider::Baidu,
            "tencent" => crate::services::image_processor::OcrProvider::Tencent,
            "tesseract" => crate::services::image_processor::OcrProvider::Tesseract,
            _ => return Err(format!("不支持的 OCR 提供商: {}", provider)),
        };
        
        let config = crate::services::image_processor::OcrConfig {
            provider: ocr_provider,
            api_key,
            secret_key: ocr_secret_key,
        };
        processor.set_ocr_config(config);
    }
    
    // 配置多模态 LLM
    if let (Some(provider), Some(api_key)) = (vision_provider, vision_api_key) {
        let vision_provider = match provider.as_str() {
            "gpt4v" => crate::services::image_processor::VisionProvider::Gpt4v,
            "qwen_vl" => crate::services::image_processor::VisionProvider::QwenVl,
            "glm4v" => crate::services::image_processor::VisionProvider::Glm4v,
            "claude" => crate::services::image_processor::VisionProvider::Claude,
            _ => return Err(format!("不支持的多模态 LLM 提供商: {}", provider)),
        };
        
        let config = crate::services::image_processor::VisionConfig {
            provider: vision_provider,
            api_key,
            base_url: vision_base_url,
        };
        processor.set_vision_config(config);
    }
    
    Ok(())
}

/// 处理单张图片
#[tauri::command]
pub async fn process_image(
    state: State<'_, AppState>,
    image_path: String,
) -> Result<ImageProcessResult, String> {
    // 克隆配置，避免在 await 时持有 MutexGuard
    let cache_dir;
    let has_ocr;
    let has_vision;
    {
        let processor = state.image_processor.lock().map_err(|e| e.to_string())?;
        has_ocr = processor.has_ocr();
        has_vision = processor.has_vision();
        cache_dir = std::env::temp_dir().join("kingdee-kb-image-cache");
    }
    
    if !has_ocr && !has_vision {
        return Err("请先配置 OCR 或多模态 LLM API".to_string());
    }
    
    // 创建新的处理器实例（配置会从 AppState 中获取）
    let processor = crate::services::image_processor::ImageProcessor::new(cache_dir);
    
    let result = processor
        .process_image(&image_path)
        .await
        .map_err(|e| e.to_string())?;
    
    Ok(ImageProcessResult {
        image_type: match result.image_type {
            crate::services::image_processor::ImageType::TextScreenshot => "text_screenshot".to_string(),
            crate::services::image_processor::ImageType::Flowchart => "flowchart".to_string(),
            crate::services::image_processor::ImageType::Architecture => "architecture".to_string(),
            crate::services::image_processor::ImageType::Table => "table".to_string(),
            crate::services::image_processor::ImageType::Mixed => "mixed".to_string(),
        },
        ocr_text: result.ocr_text,
        description: result.description,
        processing_time_ms: result.processing_time_ms,
    })
}

// ─── 响应类型 ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStatsResponse {
    pub total: usize,
    pub by_category: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillScanResult {
    pub total: usize,
    pub by_category: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDepsStatus {
    pub ocr_configured: bool,
    pub vision_configured: bool,
    pub ocr_provider: Option<String>,
    pub vision_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageProcessResult {
    pub image_type: String,
    pub ocr_text: Option<String>,
    pub description: Option<String>,
    pub processing_time_ms: u64,
}
