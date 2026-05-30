//! LLM 供应商管理 — 多供应商配置 + 自动选择
//!
//! 支持配置多个 LLM 供应商，系统根据任务类型自动选择：
//!   - 文本对话 → 用户选择的默认供应商
//!   - 图像理解 → 自动选择支持多模态的供应商
//!   - OCR → 独立的 OCR 配置

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─── 类型定义 ───

/// LLM 供应商配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMProviderConfig {
    /// 唯一标识
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 协议类型
    pub protocol: LLMProtocol,
    /// API Key
    pub api_key: String,
    /// Base URL
    pub base_url: String,
    /// 模型名称
    pub model: String,
    /// 是否为默认供应商
    pub is_default: bool,
    /// 是否支持多模态（通过 API 探测）
    #[serde(default)]
    pub is_multimodal: Option<bool>,
    /// 最后探测时间
    #[serde(default)]
    pub last_probe_at: Option<String>,
    /// 最大上下文窗口（token 数，默认：4096）
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// 生成温度（默认：0.3）
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

impl LLMProviderConfig {
    /// 检查是否已配置（有 API 密钥，或是本地模型）
    pub fn is_configured(&self) -> bool {
        if self.protocol == LLMProtocol::Local {
            return !self.base_url.is_empty();
        }
        !self.api_key.is_empty()
    }
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_temperature() -> f32 {
    0.3
}

/// LLM 协议类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LLMProtocol {
    /// OpenAI 兼容（GPT、通义千问、DeepSeek 等）
    OpenAI,
    /// Anthropic 兼容（Claude）
    Anthropic,
    /// 本地模型（Ollama、llama.cpp）
    Local,
}

/// OCR 供应商配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrProviderConfig {
    /// 唯一标识
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 供应商类型
    pub provider: OcrProviderType,
    /// API Key
    pub api_key: String,
    /// Secret Key（百度需要）
    pub secret_key: Option<String>,
    /// 是否为默认 OCR
    pub is_default: bool,
}

/// OCR 供应商类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OcrProviderType {
    Baidu,
    Tencent,
}

/// 供应商管理器
pub struct LLMProviderManager {
    /// LLM 供应商列表
    providers: Vec<LLMProviderConfig>,
    /// OCR 配置
    ocr_config: Option<OcrProviderConfig>,
    /// 配置文件路径
    config_path: PathBuf,
    /// HTTP 客户端
    client: reqwest::Client,
}

// ─── 实现 ───

impl LLMProviderManager {
    /// 创建供应商管理器
    pub fn new(data_dir: &PathBuf) -> Self {
        let config_path = data_dir.join("llm_providers.json");
        let mut manager = Self {
            providers: Vec::new(),
            ocr_config: None,
            config_path,
            client: reqwest::Client::new(),
        };
        manager.load();
        manager
    }

    /// 从文件加载配置
    fn load(&mut self) {
        if !self.config_path.exists() {
            return;
        }

        if let Ok(content) = std::fs::read_to_string(&self.config_path) {
            if let Ok(config) = serde_json::from_str::<ProviderConfigFile>(&content) {
                self.providers = config.providers.unwrap_or_default();
                self.ocr_config = config.ocr_config;
            }
        }
    }

    /// 保存配置到文件
    fn save(&self) -> Result<(), String> {
        let config = ProviderConfigFile {
            providers: Some(self.providers.clone()),
            ocr_config: self.ocr_config.clone(),
        };

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("序列化失败: {}", e))?;

        std::fs::write(&self.config_path, content)
            .map_err(|e| format!("写入失败: {}", e))?;

        Ok(())
    }

    // ─── LLM 供应商 CRUD ───

    /// 获取所有 LLM 供应商
    pub fn list_providers(&self) -> &[LLMProviderConfig] {
        &self.providers
    }

    /// 获取默认供应商
    pub fn get_default_provider(&self) -> Option<&LLMProviderConfig> {
        self.providers.iter().find(|p| p.is_default)
    }

    /// 根据 ID 获取供应商
    pub fn get_provider(&self, id: &str) -> Option<&LLMProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// 添加供应商
    pub fn add_provider(&mut self, provider: LLMProviderConfig) -> Result<(), String> {
        // 检查 ID 唯一性
        if self.providers.iter().any(|p| p.id == provider.id) {
            return Err(format!("供应商 ID '{}' 已存在", provider.id));
        }

        // 如果是第一个供应商，设为默认
        let mut provider = provider;
        if self.providers.is_empty() {
            provider.is_default = true;
        }

        self.providers.push(provider);
        self.save()
    }

    /// 更新供应商
    pub fn update_provider(&mut self, id: &str, provider: LLMProviderConfig) -> Result<(), String> {
        let index = self.providers.iter().position(|p| p.id == id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", id))?;

        self.providers[index] = provider;
        self.save()
    }

    /// 删除供应商
    pub fn delete_provider(&mut self, id: &str) -> Result<(), String> {
        let index = self.providers.iter().position(|p| p.id == id)
            .ok_or_else(|| format!("供应商 '{}' 不存在", id))?;

        let was_default = self.providers[index].is_default;
        self.providers.remove(index);

        // 如果删除的是默认供应商，将第一个设为默认
        if was_default && !self.providers.is_empty() {
            self.providers[0].is_default = true;
        }

        self.save()
    }

    /// 设置默认供应商
    pub fn set_default(&mut self, id: &str) -> Result<(), String> {
        for provider in &mut self.providers {
            provider.is_default = provider.id == id;
        }
        self.save()
    }

    // ─── OCR 配置 ───

    /// 获取 OCR 配置
    pub fn get_ocr_config(&self) -> Option<&OcrProviderConfig> {
        self.ocr_config.as_ref()
    }

    /// 设置 OCR 配置
    pub fn set_ocr_config(&mut self, config: OcrProviderConfig) -> Result<(), String> {
        self.ocr_config = Some(config);
        self.save()
    }

    /// 清除 OCR 配置
    pub fn clear_ocr_config(&mut self) -> Result<(), String> {
        self.ocr_config = None;
        self.save()
    }

    // ─── 多模态探测 ───

    /// 探测供应商是否支持多模态
    pub async fn probe_multimodal(&self, provider: &LLMProviderConfig) -> bool {
        if provider.api_key.is_empty() {
            return false;
        }

        // 用 1x1 透明图片测试
        let test_img = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

        let result = self.client
            .post(format!("{}/chat/completions", provider.base_url))
            .header("Authorization", format!("Bearer {}", provider.api_key))
            .json(&serde_json::json!({
                "model": provider.model,
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": "test"},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", test_img)}}
                ]}],
                "max_tokens": 1
            }))
            .send()
            .await;

        match result {
            Ok(resp) => {
                if resp.status().is_success() {
                    true
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    let lower = body.to_lowercase();
                    !(lower.contains("image") || lower.contains("vision") || lower.contains("multimodal"))
                }
            }
            Err(_) => false,
        }
    }

    /// 批量探测所有供应商的多模态能力
    pub async fn probe_all(&mut self) -> Vec<(String, bool)> {
        let mut results = Vec::new();

        // 克隆供应商列表以避免借用冲突
        let providers: Vec<LLMProviderConfig> = self.providers.clone();

        for provider in &providers {
            let is_multimodal = self.probe_multimodal(provider).await;
            results.push((provider.id.clone(), is_multimodal));
        }

        // 更新供应商的多模态状态
        for (id, is_multimodal) in &results {
            if let Some(provider) = self.providers.iter_mut().find(|p| &p.id == id) {
                provider.is_multimodal = Some(*is_multimodal);
                provider.last_probe_at = Some(chrono::Utc::now().to_rfc3339());
            }
        }

        let _ = self.save();
        results
    }

    // ─── 自动选择 ───

    /// 获取支持多模态的供应商（用于图像理解）
    pub fn get_multimodal_provider(&self) -> Option<&LLMProviderConfig> {
        // 优先返回默认供应商（如果支持多模态）
        if let Some(default) = self.get_default_provider() {
            if default.is_multimodal == Some(true) {
                return Some(default);
            }
        }

        // 否则返回任意支持多模态的供应商
        self.providers.iter().find(|p| p.is_multimodal == Some(true))
    }

    /// 获取供应商的 API 配置（用于 LLM 调用）
    pub fn get_provider_config(&self, id: Option<&str>) -> Option<(String, String, String)> {
        let provider = if let Some(id) = id {
            self.get_provider(id)
        } else {
            self.get_default_provider()
        };

        provider.map(|p| (p.api_key.clone(), p.base_url.clone(), p.model.clone()))
    }
}

/// 配置文件结构
#[derive(Debug, Serialize, Deserialize)]
struct ProviderConfigFile {
    #[serde(default)]
    providers: Option<Vec<LLMProviderConfig>>,
    #[serde(default)]
    ocr_config: Option<OcrProviderConfig>,
}

// ─── 测试 ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_crud() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let mut manager = LLMProviderManager::new(&data_dir);

        // 添加
        let provider = LLMProviderConfig {
            id: "test1".to_string(),
            name: "Test Provider".to_string(),
            protocol: LLMProtocol::OpenAI,
            api_key: "key1".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
            is_default: true,
            is_multimodal: None,
            last_probe_at: None,
            max_tokens: 4096,
            temperature: 0.3,
        };
        manager.add_provider(provider).unwrap();

        assert_eq!(manager.list_providers().len(), 1);
        assert!(manager.get_default_provider().is_some());

        // 更新
        let mut updated = manager.get_provider("test1").unwrap().clone();
        updated.name = "Updated".to_string();
        manager.update_provider("test1", updated).unwrap();
        assert_eq!(manager.get_provider("test1").unwrap().name, "Updated");

        // 删除
        manager.delete_provider("test1").unwrap();
        assert_eq!(manager.list_providers().len(), 0);
    }

    #[test]
    fn test_multimodal_selection() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_dir = temp_dir.path().to_path_buf();
        let mut manager = LLMProviderManager::new(&data_dir);

        // 添加两个供应商，一个支持多模态，一个不支持
        manager.add_provider(LLMProviderConfig {
            id: "text-only".to_string(),
            name: "Text Only".to_string(),
            protocol: LLMProtocol::OpenAI,
            api_key: "key1".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4".to_string(),
            is_default: true,
            is_multimodal: Some(false),
            last_probe_at: None,
            max_tokens: 4096,
            temperature: 0.3,
        }).unwrap();

        manager.add_provider(LLMProviderConfig {
            id: "multimodal".to_string(),
            name: "Multimodal".to_string(),
            protocol: LLMProtocol::OpenAI,
            api_key: "key2".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
            is_default: false,
            is_multimodal: Some(true),
            last_probe_at: None,
            max_tokens: 4096,
            temperature: 0.3,
        }).unwrap();

        // 自动选择应返回多模态供应商
        let selected = manager.get_multimodal_provider().unwrap();
        assert_eq!(selected.id, "multimodal");
    }
}
