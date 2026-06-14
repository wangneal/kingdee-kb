//! 技能系统 Tauri 命令
//!
//! 提供前端调用的技能管理接口：
//!   - list_skills / get_skill / search_skills

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

use crate::app_state::AppState;
use crate::services::prompt_assembler::SkillPromptEntry;
use crate::services::signal_writer::SignalEvent;
use crate::services::skill_executor::{ExecutionResult, SubstitutionContext};
use crate::services::skill_trigger::{SkillMatch, TriggerContext};
use crate::services::skill_types::{SharedResource, Skill, SkillFile, SkillFull};

// ─── 模板清单数据类型 ───
// 原属 services/template_manager.rs，该模块的 TemplateManager 下载器为死代码已删除，
// 仅保留这三个被 get/save_template_manifest 命令使用的清单数据类型。

/// 模板清单
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateManifest {
    pub version: String,
    pub phases: Vec<PhaseTemplates>,
}

/// 阶段模板
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseTemplates {
    pub phase: String,
    pub templates: Vec<Template>,
}

/// 单个模板
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub size: u64,
    pub checksum: String,
}

/// 列出所有技能
#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<Skill>, String> {
    let manager = state.skill_manager.lock().await;
    Ok(manager.list_all())
}

/// 按名称获取技能详情
#[tauri::command]
pub async fn get_skill(state: State<'_, AppState>, name: String) -> Result<Option<Skill>, String> {
    let manager = state.skill_manager.lock().await;
    Ok(manager.get(&name))
}

/// 搜索技能
#[tauri::command]
pub async fn search_skills(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<Skill>, String> {
    let manager = state.skill_manager.lock().await;
    Ok(manager.search(&query))
}

/// 获取技能统计
#[tauri::command]
pub async fn get_skill_stats(state: State<'_, AppState>) -> Result<SkillStatsResponse, String> {
    let manager = state.skill_manager.lock().await;
    let by_category = manager.stats().into_iter().collect();
    Ok(SkillStatsResponse {
        total: manager.count(),
        by_category,
    })
}

/// 重新扫描技能目录
#[tauri::command]
pub async fn rescan_skills(state: State<'_, AppState>) -> Result<SkillScanResult, String> {
    let mut manager = state.skill_manager.lock().await;
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
    // 短暂持锁提取触发引擎和技能映射，避免持锁期间 await 远程 embedding
    let (engine_clone, skills_snapshot) = {
        let manager = state.skill_manager.lock().await;
        let engine = manager.clone_trigger_engine();
        let skills = manager.get_skills_map();
        (engine, skills)
    };

    let engine = match engine_clone {
        Some(e) => e,
        None => return Ok(None),
    };

    let matches = engine.match_by_input(&input, &state.embedding).await;
    let best = matches.first();

    if let Some(best) = best {
        if best.score >= 3.5 {
            return Ok(skills_snapshot.get(&best.skill_id).cloned());
        }
    }
    Ok(None)
}

/// 从一个 ZIP 技能包导入新技能，解压到 skills/<skill-name>/ 目录
#[tauri::command]
pub async fn import_skill(state: State<'_, AppState>, file_path: String) -> Result<String, String> {
    let src = PathBuf::from(&file_path);

    if !src.exists() {
        return Err(format!("文件不存在: {}", file_path));
    }
    if !src.is_file() {
        return Err("路径不是文件".to_string());
    }

    let file = std::fs::File::open(&src).map_err(|e| format!("打开 ZIP 文件失败: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("解析 ZIP 压缩包失败: {}", e))?;

    // 寻找 SKILL.md 的路径并确定技能名
    let mut skill_md_entry_path = None;
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            let name = file.name();
            if name == "SKILL.md" || name.ends_with("/SKILL.md") || name.ends_with("\\SKILL.md") {
                skill_md_entry_path = Some(name.to_string());
                break;
            }
        }
    }

    let skill_md_path =
        skill_md_entry_path.ok_or_else(|| "ZIP 压缩包中未包含 SKILL.md 说明文件".to_string())?;

    // 确定前缀和技能名
    let prefix = if skill_md_path == "SKILL.md" {
        ""
    } else {
        let idx = skill_md_path.len() - "SKILL.md".len();
        &skill_md_path[..idx]
    };

    let skill_name = if prefix.is_empty() {
        src.file_stem()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "无法从 ZIP 文件名解析技能名称".to_string())?
            .to_string()
    } else {
        prefix
            .trim_end_matches('/')
            .trim_end_matches('\\')
            .to_string()
    };

    // 校验技能名合法性（仅限小写字母、数字和中划线）
    let is_valid_name = !skill_name.is_empty()
        && skill_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !is_valid_name {
        return Err(format!(
            "从压缩包解析到的技能名 [{}] 不合法，技能文件夹名只能包含小写英文字母、数字和中划线",
            skill_name
        ));
    }

    let mut manager = state.skill_manager.lock().await;
    let dest_dir = manager.get_skills_dir().join(&skill_name);
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("创建技能目录失败: {}", e))?;

    // 遍历 ZIP 包文件，进行解压与拷贝
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("读取 ZIP 内部文件项失败: {}", e))?;
        let file_name = file.name().to_string();

        if !file_name.starts_with(prefix) {
            continue;
        }

        let relative_path = &file_name[prefix.len()..];
        if relative_path.is_empty() {
            continue;
        }

        // zip-slip 防护：拒绝路径中包含 ..、绝对路径或 UNC 路径
        for component in std::path::Path::new(relative_path).components() {
            match component {
                std::path::Component::ParentDir => {
                    return Err(format!("拒绝解压路径逃逸 (ParentDir): {}", file_name));
                }
                std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                    return Err(format!("拒绝解压路径逃逸 (绝对路径): {}", file_name));
                }
                _ => {}
            }
        }

        let outpath = dest_dir.join(relative_path);
        if !outpath.starts_with(&dest_dir) {
            return Err(format!("拒绝解压路径逃逸: {}", file_name));
        }

        if file.name().ends_with('/') || file.name().ends_with('\\') {
            std::fs::create_dir_all(&outpath).map_err(|e| format!("创建子目录失败: {}", e))?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(p).map_err(|e| format!("创建子目录失败: {}", e))?;
                }
            }
            let mut outfile =
                std::fs::File::create(&outpath).map_err(|e| format!("创建解压文件失败: {}", e))?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| format!("解压拷贝失败: {}", e))?;
        }
    }

    // 重新扫描技能目录
    manager.scan();

    Ok(skill_name)
}

/// 获取技能完整信息（含支撑文件和共享资源）
#[tauri::command]
pub async fn get_skill_full(
    state: State<'_, AppState>,
    name: String,
) -> Result<Option<SkillFull>, String> {
    let manager = state.skill_manager.lock().await;
    Ok(manager.get_skill_full(&name))
}

/// 获取所有共享资源
#[tauri::command]
pub async fn list_shared_resources(
    state: State<'_, AppState>,
) -> Result<Vec<SharedResource>, String> {
    let manager = state.skill_manager.lock().await;
    Ok(manager.get_shared_resources())
}

/// 读取技能支撑文件内容
#[tauri::command]
pub async fn read_skill_file(
    state: State<'_, AppState>,
    skill_name: String,
    relative_path: String,
) -> Result<String, String> {
    let manager = state.skill_manager.lock().await;
    manager.read_skill_file(&skill_name, &relative_path)
}

/// 获取技能支撑文件列表
#[tauri::command]
pub async fn list_skill_files(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<SkillFile>, String> {
    let manager = state.skill_manager.lock().await;
    Ok(manager.get_skill_files(&name))
}

// ─── Phase 2: 触发匹配命令 ──────────────────────────────────

/// 触发技能匹配（使用完整触发上下文）
#[tauri::command]
pub async fn trigger_skill_match(
    state: State<'_, AppState>,
    context: TriggerContext,
) -> Result<Vec<SkillMatch>, String> {
    // 短暂持锁提取触发引擎，避免持锁期间 await 远程 embedding
    let engine_clone = {
        let manager = state.skill_manager.lock().await;
        manager.clone_trigger_engine()
    };

    let mut matches = Vec::new();

    if let Some(ref engine) = engine_clone {
        // 使用触发引擎匹配（仅调用一次 match_by_input，避免重复 embedding 请求）
        let mut all_matches = engine
            .match_by_input(&context.user_input, &state.embedding)
            .await;
        // 合并路径匹配
        let path_matches = engine.match_by_paths(&context.accessed_files);
        all_matches.extend(path_matches);
        // 使用 total_cmp 避免 NaN 导致 panic
        all_matches.sort_by(|a, b| b.score.total_cmp(&a.score));

        // 取最佳匹配
        if let Some(m) = all_matches.first() {
            matches.push(m.clone());
        }

        // 补充候选（排除已选中的最佳匹配）
        for candidate in all_matches.iter().take(6).skip(1) {
            if !matches.iter().any(|m| m.skill_id == candidate.skill_id) {
                matches.push(candidate.clone());
            }
        }
    }

    // 记录匹配事件
    if let Some(first) = matches.first() {
        if let Ok(writer) = state.signal_writer.write() {
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
    // 短暂持锁提取触发引擎，避免持锁期间 await 远程 embedding
    let engine_clone = {
        let manager = state.skill_manager.lock().await;
        manager.clone_trigger_engine()
    };

    let engine = match engine_clone {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };

    let mut matches = engine.match_by_input(&input, &state.embedding).await;
    matches.truncate(limit.unwrap_or(5));
    Ok(matches)
}

/// 生成技能列表系统提示
#[tauri::command]
pub async fn get_skill_list_prompt(state: State<'_, AppState>) -> Result<String, String> {
    let manager = state.skill_manager.lock().await;
    Ok(manager.build_skill_list_prompt())
}

/// 获取技能摘要列表（用于前端展示和提示注入）
#[tauri::command]
pub async fn get_skill_prompt_entries(
    state: State<'_, AppState>,
) -> Result<Vec<SkillPromptEntry>, String> {
    let manager = state.skill_manager.lock().await;
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
        let manager = state.skill_manager.lock().await;
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
    if let Ok(writer) = state.signal_writer.write() {
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

    let content =
        std::fs::read_to_string(&manifest_path).map_err(|e| format!("读取模板清单失败: {}", e))?;

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

    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("序列化模板清单失败: {}", e))?;

    std::fs::write(&manifest_path, content).map_err(|e| format!("写入模板清单失败: {}", e))?;

    Ok(())
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
