//! 模板管理器 — 从 Gitee 下载和缓存 Office 模板
//!
//! 参考 Claude Code 技能系统的模板懒加载机制：
//!   - 从 Gitee 私有仓库下载模板
//!   - SHA256 校验
//!   - 本地缓存

use std::path::PathBuf;

/// 模板清单
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemplateManifest {
    pub version: String,
    pub phases: Vec<PhaseTemplates>,
}

/// 阶段模板
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PhaseTemplates {
    pub phase: String,
    pub templates: Vec<Template>,
}

/// 单个模板
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub size: u64,
    pub checksum: String,
}

/// 模板管理器
pub struct TemplateManager {
    /// Gitee API 基础 URL
    base_url: String,
    /// 访问令牌
    access_token: String,
    /// 本地缓存目录
    cache_dir: PathBuf,
    /// 模板清单
    manifest: Option<TemplateManifest>,
}

impl TemplateManager {
    /// 创建模板管理器
    pub fn new(cache_dir: PathBuf, access_token: String) -> Self {
        Self {
            base_url: "https://gitee.com/api/v5".to_string(),
            access_token,
            cache_dir,
            manifest: None,
        }
    }

    /// 加载模板清单
    pub async fn load_manifest(&mut self, repo: &str, path: &str) -> Result<(), TemplateError> {
        let url = format!(
            "{}/repos/{}/contents/{}?access_token={}",
            self.base_url, repo, path, self.access_token
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| TemplateError::DownloadFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(TemplateError::DownloadFailed(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let content: serde_json::Value = response
            .json()
            .await
            .map_err(|e| TemplateError::ParseError(e.to_string()))?;

        // 解析 Gitee API 响应
        let manifest: TemplateManifest = serde_json::from_value(content)
            .map_err(|e| TemplateError::ParseError(e.to_string()))?;

        self.manifest = Some(manifest);
        Ok(())
    }

    /// 下载模板
    pub async fn download_template(&self, template_id: &str) -> Result<PathBuf, TemplateError> {
        let manifest = self.manifest.as_ref().ok_or(TemplateError::ManifestNotLoaded)?;

        // 查找模板
        let template = manifest
            .phases
            .iter()
            .flat_map(|p| &p.templates)
            .find(|t| t.id == template_id)
            .ok_or_else(|| TemplateError::TemplateNotFound(template_id.to_string()))?;

        let cache_path = self.cache_dir.join(&template.name);

        // 检查缓存
        if cache_path.exists() {
            let checksum = self.compute_checksum(&cache_path)?;
            if checksum == template.checksum {
                return Ok(cache_path);
            }
        }

        // 下载
        let client = reqwest::Client::new();
        let response = client
            .get(&template.url)
            .header("Authorization", format!("token {}", self.access_token))
            .send()
            .await
            .map_err(|e| TemplateError::DownloadFailed(e.to_string()))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| TemplateError::DownloadFailed(e.to_string()))?;

        // 校验
        let checksum = self.compute_sha256(&bytes);
        if checksum != template.checksum {
            return Err(TemplateError::ChecksumMismatch);
        }

        // 写入缓存
        std::fs::create_dir_all(&self.cache_dir)
            .map_err(|e| TemplateError::IoError(e.to_string()))?;

        std::fs::write(&cache_path, &bytes)
            .map_err(|e| TemplateError::IoError(e.to_string()))?;

        Ok(cache_path)
    }

    /// 批量下载阶段模板
    pub async fn download_phase_templates(
        &self,
        phase: &str,
    ) -> Result<Vec<PathBuf>, TemplateError> {
        let manifest = self.manifest.as_ref().ok_or(TemplateError::ManifestNotLoaded)?;

        let phase_templates = manifest
            .phases
            .iter()
            .find(|p| p.phase == phase)
            .ok_or_else(|| TemplateError::PhaseNotFound(phase.to_string()))?;

        let mut results = Vec::new();
        for template in &phase_templates.templates {
            let path = self.download_template(&template.id).await?;
            results.push(path);
        }

        Ok(results)
    }

    /// 获取模板列表
    pub fn list_templates(&self) -> Vec<&Template> {
        self.manifest
            .as_ref()
            .map(|m| m.phases.iter().flat_map(|p| &p.templates).collect())
            .unwrap_or_default()
    }

    /// 获取阶段模板列表
    pub fn list_phase_templates(&self, phase: &str) -> Vec<&Template> {
        self.manifest
            .as_ref()
            .and_then(|m| m.phases.iter().find(|p| p.phase == phase))
            .map(|p| p.templates.iter().collect())
            .unwrap_or_default()
    }

    /// 计算文件 SHA256
    fn compute_checksum(&self, path: &PathBuf) -> Result<String, TemplateError> {
        let content = std::fs::read(path).map_err(|e| TemplateError::IoError(e.to_string()))?;
        Ok(self.compute_sha256(&content))
    }

    /// 计算 SHA256
    fn compute_sha256(&self, data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }
}

/// 模板错误
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("下载失败: {0}")]
    DownloadFailed(String),

    #[error("解析失败: {0}")]
    ParseError(String),

    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("清单未加载")]
    ManifestNotLoaded,

    #[error("模板不存在: {0}")]
    TemplateNotFound(String),

    #[error("阶段不存在: {0}")]
    PhaseNotFound(String),

    #[error("校验和不匹配")]
    ChecksumMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_serialization() {
        let manifest = TemplateManifest {
            version: "1.0".to_string(),
            phases: vec![PhaseTemplates {
                phase: "01_启动阶段".to_string(),
                templates: vec![Template {
                    id: "kickoff_ppt".to_string(),
                    name: "启动会PPT模板.pptx".to_string(),
                    description: "项目启动会PPT模板".to_string(),
                    url: "https://example.com/kickoff.pptx".to_string(),
                    size: 1024,
                    checksum: "abc123".to_string(),
                }],
            }],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("kickoff_ppt"));
        assert!(json.contains("01_启动阶段"));
    }
}
