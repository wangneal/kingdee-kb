//! 技能加载器 — 支撑文件、共享资源、完整技能加载
//!
//! 与 skill_manager.rs 互补：
//!   - skill_manager 负责扫描/缓存/搜索基础 Skill
//!   - skill_loader 负责加载支撑文件（scripts/references/assets）和 _shared 资源

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::services::skill_types::{
    parse_skill_md, SharedResource, Skill, SkillFile, SkillFileType, SkillFull,
};

/// 技能加载器（无状态，所有方法均为纯函数）
pub struct SkillLoader;

impl SkillLoader {
    /// 扫描技能目录下的支撑文件
    ///
    /// 分类规则：
    ///   - scripts/ 目录下 -> Script
    ///   - references/ 目录下 -> Reference
    ///   - assets/ 目录下 -> Asset
    ///   - *.json, *.yaml, *.yml, *.toml -> Config
    ///   - 其他文件 -> Other
    ///
    /// 跳过 SKILL.md 本身和子目录。
    pub fn load_supporting_files(dir: &Path) -> Vec<SkillFile> {
        let mut files = Vec::new();
        Self::collect_files_recursive(dir, dir, &mut files);
        files
    }

    /// 递归收集文件（排除 SKILL.md 和隐藏目录）
    fn collect_files_recursive(base: &Path, current: &Path, files: &mut Vec<SkillFile>) {
        let entries = match fs::read_dir(current) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();

            // 跳过隐藏目录
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') || name.starts_with('_') {
                        continue;
                    }
                }
                Self::collect_files_recursive(base, &path, files);
                continue;
            }

            // 跳过 SKILL.md
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "SKILL.md" {
                    continue;
                }
            }

            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let relative_path = match path.strip_prefix(base) {
                Ok(p) => p.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let file_type = Self::classify_file(&relative_path, &name);

            let last_modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            files.push(SkillFile {
                path: relative_path,
                name,
                file_type,
                size: metadata.len(),
                last_modified,
            });
        }
    }

    /// 根据路径和文件名判断文件类型
    fn classify_file(relative_path: &str, name: &str) -> SkillFileType {
        let path_lower = relative_path.to_lowercase();

        // 按目录分类
        if path_lower.starts_with("scripts/") || path_lower.contains("/scripts/") {
            return SkillFileType::Script;
        }
        if path_lower.starts_with("references/") || path_lower.contains("/references/") {
            return SkillFileType::Reference;
        }
        if path_lower.starts_with("assets/") || path_lower.contains("/assets/") {
            return SkillFileType::Asset;
        }

        // 按扩展名分类
        if let Some(ext) = name.rsplit('.').next() {
            match ext.to_lowercase().as_str() {
                "json" | "yaml" | "yml" | "toml" => return SkillFileType::Config,
                "md" | "txt" => return SkillFileType::Reference,
                "py" | "sh" | "js" | "ts" | "ps1" | "bat" | "cmd" => return SkillFileType::Script,
                "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" => {
                    return SkillFileType::Asset
                }
                _ => {}
            }
        }

        SkillFileType::Other
    }

    /// 加载 _shared/ 目录下的共享资源
    ///
    /// - 跳过 _shared/scripts/（Python 脚本，非文本可读）
    /// - 读取所有 .md 文件内容
    pub fn load_shared_resources(skills_dir: &Path) -> Vec<SharedResource> {
        let shared_dir = skills_dir.join("_shared");
        if !shared_dir.exists() || !shared_dir.is_dir() {
            return Vec::new();
        }

        let mut resources = Vec::new();
        Self::collect_shared_recursive(&shared_dir, &shared_dir, &mut resources);
        resources
    }

    /// 递归收集 _shared/ 下的资源
    fn collect_shared_recursive(base: &Path, current: &Path, resources: &mut Vec<SharedResource>) {
        let entries = match fs::read_dir(current) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                // 跳过 scripts 目录
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name == "scripts" {
                        continue;
                    }
                }
                Self::collect_shared_recursive(base, &path, resources);
                continue;
            }

            let relative_path = match path.strip_prefix(base) {
                Ok(p) => p.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // 只处理 .md 文件
            if !name.to_lowercase().ends_with(".md") {
                continue;
            }

            let content = fs::read_to_string(&path).ok();

            resources.push(SharedResource {
                name,
                path: relative_path,
                content,
            });
        }
    }

    /// 加载完整技能（含支撑文件和共享资源）
    ///
    /// 1. 读取 SKILL.md，解析 frontmatter + body
    /// 2. 收集支撑文件
    /// 3. 关联匹配的共享资源
    pub fn load_skill_full(dir: &Path, shared: &[SharedResource]) -> Option<SkillFull> {
        let skill_md = dir.join("SKILL.md");
        if !skill_md.exists() {
            return None;
        }

        let content = fs::read_to_string(&skill_md).ok()?;
        let (metadata, body) = parse_skill_md(&content);

        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let location = skill_md.to_string_lossy().to_string();

        let scripts = Self::list_dir_names(&dir.join("scripts"));
        let references = Self::list_dir_names(&dir.join("references"));

        let skill = Skill {
            name,
            location,
            metadata,
            body,
            scripts,
            references,
        };

        let supporting_files = Self::load_supporting_files(dir);

        // 关联共享资源：将所有 shared 资源关联到技能
        let shared_references = shared.to_vec();

        Some(SkillFull {
            skill,
            supporting_files,
            shared_references,
        })
    }

    /// 读取技能目录下的指定支撑文件
    ///
    /// 安全措施：canonicalize 验证路径防止目录遍历攻击
    pub fn read_skill_file(skill_dir: &Path, relative_path: &str) -> Result<String, String> {
        // 拒绝空路径
        if relative_path.is_empty() {
            return Err("文件路径不能为空".to_string());
        }

        // 拒绝绝对路径
        if Path::new(relative_path).is_absolute() {
            return Err("不支持绝对路径".to_string());
        }

        // 拒绝路径遍历
        if relative_path.contains("..") {
            return Err("路径包含 '..'，拒绝访问".to_string());
        }

        let target = skill_dir.join(relative_path);

        // canonicalize 验证
        let canonical_skill_dir = skill_dir
            .canonicalize()
            .map_err(|e| format!("无法解析技能目录: {}", e))?;

        let canonical_target = target
            .canonicalize()
            .map_err(|e| format!("文件不存在或无法访问: {}", e))?;

        if !canonical_target.starts_with(&canonical_skill_dir) {
            return Err("路径超出技能目录范围".to_string());
        }

        fs::read_to_string(&canonical_target).map_err(|e| format!("读取文件失败: {}", e))
    }

    /// 列出目录下的文件名
    fn list_dir_names(dir: &Path) -> Vec<String> {
        if !dir.exists() {
            return Vec::new();
        }
        fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                if e.path().is_file() {
                    e.file_name().to_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_skill(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            r#"---
name: test-skill
description: "A test skill"
version: "1.0"
category: tool
---

# Test Skill

Some content here."#,
        )
        .unwrap();

        // scripts/
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("run.sh"), "#!/bin/bash\necho hello").unwrap();

        // references/
        let refs = dir.join("references");
        fs::create_dir_all(&refs).unwrap();
        fs::write(refs.join("guide.md"), "# Guide\nSome reference").unwrap();

        // assets/
        let assets = dir.join("assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("logo.png"), [0u8; 100]).unwrap();

        // config file at root
        fs::write(dir.join("config.json"), r#"{"key": "value"}"#).unwrap();
    }

    #[test]
    fn test_load_supporting_files() {
        let tmp = std::env::temp_dir().join("skill_loader_test_supporting");
        let _ = fs::remove_dir_all(&tmp);
        create_test_skill(&tmp);

        let files = SkillLoader::load_supporting_files(&tmp);
        assert!(files.len() >= 4, "expected >=4 files, got {}", files.len());

        let scripts: Vec<_> = files
            .iter()
            .filter(|f| f.file_type == SkillFileType::Script)
            .collect();
        assert!(!scripts.is_empty(), "should have script files");

        let refs: Vec<_> = files
            .iter()
            .filter(|f| f.file_type == SkillFileType::Reference)
            .collect();
        assert!(!refs.is_empty(), "should have reference files");

        let assets: Vec<_> = files
            .iter()
            .filter(|f| f.file_type == SkillFileType::Asset)
            .collect();
        assert!(!assets.is_empty(), "should have asset files");

        let configs: Vec<_> = files
            .iter()
            .filter(|f| f.file_type == SkillFileType::Config)
            .collect();
        assert!(!configs.is_empty(), "should have config files");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_skill_full() {
        let tmp = std::env::temp_dir().join("skill_loader_test_full");
        let _ = fs::remove_dir_all(&tmp);
        create_test_skill(&tmp);

        let shared = vec![SharedResource {
            name: "common.md".to_string(),
            path: "common.md".to_string(),
            content: Some("# Common".to_string()),
        }];

        let full = SkillLoader::load_skill_full(&tmp, &shared).expect("should load skill full");
        assert_eq!(full.skill.name, "skill_loader_test_full");
        assert_eq!(full.skill.metadata.name, Some("test-skill".to_string()));
        assert!(!full.supporting_files.is_empty());
        assert_eq!(full.shared_references.len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_skill_file_valid() {
        let tmp = std::env::temp_dir().join("skill_loader_test_read");
        let _ = fs::remove_dir_all(&tmp);
        create_test_skill(&tmp);

        let content = SkillLoader::read_skill_file(&tmp, "scripts/run.sh");
        assert!(content.is_ok());
        assert!(content.unwrap().contains("echo hello"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_skill_file_traversal_blocked() {
        let tmp = std::env::temp_dir().join("skill_loader_test_traversal");
        let _ = fs::remove_dir_all(&tmp);
        create_test_skill(&tmp);

        // 路径遍历应被拒绝
        let result = SkillLoader::read_skill_file(&tmp, "../other/file.txt");
        assert!(result.is_err());

        let result = SkillLoader::read_skill_file(&tmp, "scripts/../../etc/passwd");
        assert!(result.is_err());

        // 空路径
        let result = SkillLoader::read_skill_file(&tmp, "");
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_classify_file() {
        assert_eq!(
            SkillLoader::classify_file("scripts/run.sh", "run.sh"),
            SkillFileType::Script
        );
        assert_eq!(
            SkillLoader::classify_file("references/guide.md", "guide.md"),
            SkillFileType::Reference
        );
        assert_eq!(
            SkillLoader::classify_file("assets/logo.png", "logo.png"),
            SkillFileType::Asset
        );
        assert_eq!(
            SkillLoader::classify_file("config.json", "config.json"),
            SkillFileType::Config
        );
        assert_eq!(
            SkillLoader::classify_file("readme.txt", "readme.txt"),
            SkillFileType::Reference
        );
        assert_eq!(
            SkillLoader::classify_file("data.bin", "data.bin"),
            SkillFileType::Other
        );
    }

    #[test]
    fn test_load_shared_resources() {
        let tmp = std::env::temp_dir().join("skill_loader_test_shared");
        let _ = fs::remove_dir_all(&tmp);

        let shared_dir = tmp.join("_shared");
        fs::create_dir_all(&shared_dir).unwrap();
        fs::write(shared_dir.join("common.md"), "# Common Reference").unwrap();

        // scripts 子目录应被跳过
        let scripts_dir = shared_dir.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        fs::write(scripts_dir.join("helper.py"), "print('hello')").unwrap();

        // 非 .md 文件应被跳过
        fs::write(shared_dir.join("data.json"), "{}").unwrap();

        let resources = SkillLoader::load_shared_resources(&tmp);
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].name, "common.md");
        assert!(resources[0].content.as_ref().unwrap().contains("Common"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
