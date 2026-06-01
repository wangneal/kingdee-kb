// Integration tests for the multi-model vision pipeline.
//
// Covers:
//   - builtin_supports_vision lookup
//   - 3-tier candidate filtering (probed → builtin DB → unknown fallback)
//   - Field-level metadata merge via resolve_metadata
//   - Fallback chain logic
//   - Local protocol (Ollama) empty API key handling

use kingdee_kb_lib::services::llm_providers::{
    anthropic_messages_url, ApiKeyConfig, LLMProtocol, LLMProviderConfig, LLMProviderManager,
    ModelConfig,
};
use kingdee_kb_lib::services::model_metadata::{builtin_supports_vision, resolve_metadata};

mod vision_pipeline {
    use super::*;

    // ─── Helpers ────────────────────────────────────────────────────────────

    fn make_provider(
        id: &str,
        name: &str,
        protocol: LLMProtocol,
        base_url: &str,
        models: Vec<ModelConfig>,
    ) -> LLMProviderConfig {
        LLMProviderConfig {
            id: id.into(),
            name: name.into(),
            protocol,
            base_url: base_url.into(),
            is_default: false,
            api_keys: vec![ApiKeyConfig {
                id: format!("{}-key", id),
                name: format!("{} key", name),
                key: format!("sk-{}", id),
                is_default: true,
            }],
            models,
            max_tokens: 4096,
            temperature: 0.3,
            // legacy fields
            api_key: String::new(),
            model: String::new(),
            is_multimodal: None,
            last_probe_at: None,
        }
    }

    fn make_model(id: &str, name: &str, is_multimodal: Option<bool>) -> ModelConfig {
        ModelConfig {
            id: id.into(),
            name: name.into(),
            is_default: true,
            is_multimodal,
            last_probe_at: None,
            context_window: None,
            max_output_tokens: None,
            supports_thinking: None,
        }
    }

    /// Create a manager with providers covering all three tiers:
    ///   - openai/gpt-4o: is_multimodal = Some(true)  → tier 1
    ///   - anthropic/claude-sonnet-4-5: is_multimodal = None, builtin vision=true → tier 2
    ///   - deepseek/deepseek-v4-pro: is_multimodal = None, builtin vision=false → excluded
    fn make_full_manager() -> LLMProviderManager {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = LLMProviderManager::new(&tmp.keep().into());

        // Provider 1: OpenAI — probed multimodal
        mgr.add_provider(make_provider(
            "openai",
            "OpenAI",
            LLMProtocol::OpenAI,
            "https://api.openai.com/v1",
            vec![make_model("m-openai", "gpt-4o", Some(true))],
        ))
        .unwrap();

        // Provider 2: DeepSeek — not probed, builtin says vision=false
        mgr.add_provider(make_provider(
            "deepseek",
            "DeepSeek",
            LLMProtocol::OpenAI,
            "https://api.deepseek.com/v1",
            vec![make_model("m-ds", "deepseek-v4-pro", None)],
        ))
        .unwrap();

        // Provider 3: Anthropic — not probed, builtin says vision=true
        mgr.add_provider(make_provider(
            "anthropic",
            "Anthropic",
            LLMProtocol::Anthropic,
            "https://api.anthropic.com",
            vec![make_model("m-ant", "claude-sonnet-4-5", None)],
        ))
        .unwrap();

        mgr
    }

    // ─── Test 1: builtin_supports_vision — known models ─────────────────────

    #[test]
    fn test_builtin_supports_vision_known_models() {
        // Models with supports_vision: true in model_specs.json
        assert_eq!(builtin_supports_vision("gpt-4o"), Some(true));
        assert_eq!(builtin_supports_vision("gpt-5"), Some(true));
        assert_eq!(builtin_supports_vision("o3"), Some(true));
        assert_eq!(builtin_supports_vision("o4-mini"), Some(true));
        assert_eq!(builtin_supports_vision("claude-opus-4-7"), Some(true));
        assert_eq!(builtin_supports_vision("claude-sonnet-4-5"), Some(true));

        // Models with supports_vision: false
        assert_eq!(builtin_supports_vision("deepseek-v4-pro"), Some(false));
        assert_eq!(builtin_supports_vision("deepseek-v4-flash"), Some(false));
        assert_eq!(builtin_supports_vision("deepseek-reasoner"), Some(false));
    }

    #[test]
    fn test_builtin_supports_vision_unknown_model() {
        assert_eq!(builtin_supports_vision("nonexistent-model-xyz"), None);
        assert_eq!(builtin_supports_vision(""), None);
    }

    // ─── Test 2: get_vision_candidates — tier 1 (probed multimodal) ─────────

    #[test]
    fn test_vision_candidates_tier1_probed() {
        let mgr = make_full_manager();
        let candidates = mgr.get_vision_candidates();

        // gpt-4o has is_multimodal = Some(true), should appear
        assert!(
            candidates.iter().any(|c| c.2 == "gpt-4o"),
            "Tier 1: probed multimodal model gpt-4o should be a vision candidate"
        );
    }

    // ─── Test 3: get_vision_candidates — merged list with ordering ───────────

    #[test]
    fn test_vision_candidates_excludes_non_vision() {
        let mgr = make_full_manager();
        let candidates = mgr.get_vision_candidates();

        // deepseek-v4-pro has is_multimodal = None AND builtin supports_vision = false.
        // Tier 3 now excludes models where builtin DB explicitly says supports_vision=false.
        assert!(
            !candidates.iter().any(|c| c.2 == "deepseek-v4-pro"),
            "deepseek-v4-pro should NOT appear — builtin DB explicitly marks it as non-vision"
        );

        // Only vision-capable candidates should remain: gpt-4o (tier 1), claude-sonnet-4-5 (tier 2)
        assert!(
            candidates.iter().any(|c| c.2 == "gpt-4o"),
            "gpt-4o should be present"
        );
        assert!(
            candidates.iter().any(|c| c.2 == "claude-sonnet-4-5"),
            "claude-sonnet-4-5 should be present"
        );
    }

    // ─── Test 4: get_vision_candidates — tier 2 (builtin DB fallback) ───────

    #[test]
    fn test_vision_candidates_tier2_builtin_db() {
        // Build a manager with NO probed models so tier 1 is empty.
        // Now all tiers run and are merged, so tier 3 models also appear.
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = LLMProviderManager::new(&tmp.keep().into());

        // claude-sonnet-4-5: is_multimodal=None, builtin supports_vision=true → tier 2
        mgr.add_provider(make_provider(
            "anthropic",
            "Anthropic",
            LLMProtocol::Anthropic,
            "https://api.anthropic.com",
            vec![make_model("m-ant", "claude-sonnet-4-5", None)],
        ))
        .unwrap();

        // deepseek-v4-pro: is_multimodal=None, builtin supports_vision=false → excluded from all tiers
        mgr.add_provider(make_provider(
            "deepseek",
            "DeepSeek",
            LLMProtocol::OpenAI,
            "https://api.deepseek.com/v1",
            vec![make_model("m-ds", "deepseek-v4-pro", None)],
        ))
        .unwrap();

        let candidates = mgr.get_vision_candidates();

        assert!(
            candidates.iter().any(|c| c.2 == "claude-sonnet-4-5"),
            "Tier 2: claude-sonnet-4-5 should be picked up via builtin DB"
        );

        // deepseek-v4-pro is excluded: builtin DB says supports_vision=false
        assert!(
            !candidates.iter().any(|c| c.2 == "deepseek-v4-pro"),
            "deepseek-v4-pro should NOT appear — builtin DB marks it as non-vision"
        );
    }

    // ─── Test 5: get_vision_candidates — tier 3 (unknown fallback) ──────────

    #[test]
    fn test_vision_candidates_tier3_unknown_models() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = LLMProviderManager::new(&tmp.keep().into());

        // A model NOT in builtin DB and not probed → tier 3 includes it
        mgr.add_provider(make_provider(
            "custom",
            "CustomLLM",
            LLMProtocol::OpenAI,
            "https://custom-llm.example.com/v1",
            vec![make_model("m-custom", "my-custom-vision-model", None)],
        ))
        .unwrap();

        // A model explicitly marked non-multimodal → excluded even from tier 3
        mgr.add_provider(make_provider(
            "textonly",
            "TextOnly",
            LLMProtocol::OpenAI,
            "https://text.example.com/v1",
            vec![make_model("m-txt", "gpt-3.5-turbo", Some(false))],
        ))
        .unwrap();

        let candidates = mgr.get_vision_candidates();

        assert!(
            candidates.iter().any(|c| c.2 == "my-custom-vision-model"),
            "Tier 3: unknown model should be included as fallback candidate"
        );
        assert!(
            !candidates.iter().any(|c| c.2 == "gpt-3.5-turbo"),
            "Tier 3: explicitly non-multimodal model should be excluded"
        );
    }

    // ─── Test 6: get_vision_candidates — empty when all non-multimodal ───────

    #[test]
    fn test_vision_candidates_empty_when_all_explicitly_non_multimodal() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = LLMProviderManager::new(&tmp.keep().into());

        mgr.add_provider(make_provider(
            "p1",
            "Provider1",
            LLMProtocol::OpenAI,
            "https://api1.example.com/v1",
            vec![make_model("m1", "gpt-4o", Some(false))],
        ))
        .unwrap();

        mgr.add_provider(make_provider(
            "p2",
            "Provider2",
            LLMProtocol::OpenAI,
            "https://api2.example.com/v1",
            vec![make_model("m2", "claude-sonnet-4-5", Some(false))],
        ))
        .unwrap();

        let candidates = mgr.get_vision_candidates();

        assert!(
            candidates.is_empty(),
            "When all models are explicitly non-multimodal, candidates should be empty"
        );
    }

    // ─── Test 7: get_vision_candidates — protocol is correctly included ──────

    #[test]
    fn test_vision_candidates_include_protocol() {
        let mgr = make_full_manager();
        let candidates = mgr.get_vision_candidates();

        let openai_candidate = candidates.iter().find(|c| c.2 == "gpt-4o").unwrap();
        assert_eq!(
            openai_candidate.5,
            LLMProtocol::OpenAI,
            "gpt-4o candidate should carry OpenAI protocol"
        );
    }

    // ─── Test 8: resolve_metadata — field-level merge preserves builtin vision

    #[tokio::test]
    async fn test_resolve_metadata_preserves_builtin_vision() {
        // Construct a provider with claude-sonnet-4-5 where:
        //   - user sets context_window override (Some(100000))
        //   - is_multimodal is NOT set (None) → should NOT override builtin supports_vision
        let provider = make_provider(
            "anthropic",
            "Anthropic",
            LLMProtocol::OpenAI, // non-anthropic URL → from_provider_api returns None immediately
            "https://api.openai.com/v1",
            vec![ModelConfig {
                id: "m-ant".into(),
                name: "claude-sonnet-4-5".into(),
                is_default: true,
                is_multimodal: None, // NOT set by user
                last_probe_at: None,
                context_window: Some(100_000), // user override
                max_output_tokens: None,
                supports_thinking: None,
            }],
        );

        let meta = resolve_metadata(&provider, "claude-sonnet-4-5").await;

        // supports_vision should come from builtin DB (true) since user did not set is_multimodal
        assert!(
            meta.supports_vision,
            "supports_vision should be true from builtin DB when user does not override is_multimodal"
        );

        // context_window should be the user override (100000), not the builtin value (200000)
        assert_eq!(
            meta.context_window, 100_000,
            "context_window should reflect user override"
        );

        // Other builtin values should be preserved
        assert_eq!(
            meta.max_output_tokens, 64_000,
            "max_output_tokens from builtin DB"
        );
        assert!(meta.supports_thinking, "supports_thinking from builtin DB");
        assert!(meta.supports_tools, "supports_tools from builtin DB");
    }

    // ─── Test 9: resolve_metadata — user is_multimodal overrides builtin ─────

    #[tokio::test]
    async fn test_resolve_metadata_user_multimodal_override() {
        // When user explicitly sets is_multimodal = Some(false), it should override builtin
        let provider = make_provider(
            "openai",
            "OpenAI",
            LLMProtocol::OpenAI,
            "https://api.openai.com/v1",
            vec![ModelConfig {
                id: "m-4o".into(),
                name: "gpt-4o".into(),
                is_default: true,
                is_multimodal: Some(false), // user explicitly says NOT multimodal
                last_probe_at: None,
                context_window: None,
                max_output_tokens: None,
                supports_thinking: None,
            }],
        );

        let meta = resolve_metadata(&provider, "gpt-4o").await;

        // User override: is_multimodal = Some(false) → supports_vision = false
        assert!(
            !meta.supports_vision,
            "When user sets is_multimodal=false, supports_vision should be false even though builtin says true"
        );
    }

    // ─── Test 10: get_vision_candidates — tier ordering ──────────────────────

    #[test]
    fn test_vision_candidates_tier1_takes_precedence() {
        // All tiers are merged. Tier 1 models appear first, then tier 2, then tier 3.
        let mgr = make_full_manager();
        let candidates = mgr.get_vision_candidates();

        // gpt-4o (tier 1) should appear BEFORE claude-sonnet-4-5 (tier 2)
        let gpt4o_pos = candidates.iter().position(|c| c.2 == "gpt-4o").unwrap();
        let claude_pos = candidates
            .iter()
            .position(|c| c.2 == "claude-sonnet-4-5")
            .unwrap();
        assert!(
            gpt4o_pos < claude_pos,
            "tier 1 candidates should appear before tier 2"
        );

        // Both should be present
        assert!(
            candidates.len() >= 2,
            "Should have at least 2 candidates (tier 1 + tier 2)"
        );
    }

    // ─── Test 11: Local (Ollama) with empty API key appears in candidates ────

    #[test]
    fn test_vision_candidates_local_empty_key() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = LLMProviderManager::new(&tmp.keep().into());

        // Local Ollama provider with empty API key
        let local_provider = LLMProviderConfig {
            id: "ollama".into(),
            name: "Ollama".into(),
            protocol: LLMProtocol::Local,
            base_url: "http://localhost:11434/v1".into(),
            is_default: true,
            api_keys: vec![ApiKeyConfig {
                id: "ollama-key".into(),
                name: "Ollama key".into(),
                key: String::new(), // empty key
                is_default: true,
            }],
            models: vec![make_model("llava", "llava:latest", None)],
            max_tokens: 4096,
            temperature: 0.3,
            api_key: String::new(),
            model: String::new(),
            is_multimodal: None,
            last_probe_at: None,
        };

        let _ = mgr.add_provider(local_provider);

        let candidates = mgr.get_vision_candidates();

        // Local provider with empty key should still appear as a candidate
        let llava = candidates.iter().find(|c| c.2 == "llava:latest");
        assert!(
            llava.is_some(),
            "Local (Ollama) provider with empty API key should appear in vision candidates"
        );

        let (api_key, _, _, _, _, protocol) = llava.unwrap();
        assert!(
            api_key.is_empty(),
            "Local provider should have empty API key"
        );
        assert_eq!(
            *protocol,
            LLMProtocol::Local,
            "Local provider should carry Local protocol"
        );
    }

    // ─── Test 12: Remote provider with empty key still in candidates (caller filters) ──

    #[test]
    fn test_vision_candidates_remote_empty_key_protocol_not_local() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = LLMProviderManager::new(&tmp.keep().into());

        // Remote OpenAI provider with empty API key
        let remote_provider = LLMProviderConfig {
            id: "openai".into(),
            name: "OpenAI".into(),
            protocol: LLMProtocol::OpenAI,
            base_url: "https://api.openai.com/v1".into(),
            is_default: true,
            api_keys: vec![ApiKeyConfig {
                id: "openai-key".into(),
                name: "OpenAI key".into(),
                key: String::new(), // empty key
                is_default: true,
            }],
            models: vec![make_model("gpt-4o", "gpt-4o", None)],
            max_tokens: 4096,
            temperature: 0.3,
            api_key: String::new(),
            model: String::new(),
            is_multimodal: None,
            last_probe_at: None,
        };

        let _ = mgr.add_provider(remote_provider);

        let candidates = mgr.get_vision_candidates();

        // get_vision_candidates() returns the candidate — it's the caller (skill.rs) that
        // filters by api_key.is_empty() && protocol != Local.
        // Verify the candidate carries the right protocol so the caller can filter.
        let gpt4o = candidates.iter().find(|c| c.2 == "gpt-4o");
        assert!(
            gpt4o.is_some(),
            "Remote provider should appear in candidates (caller filters by key)"
        );

        let (api_key, _, _, _, _, protocol) = gpt4o.unwrap();
        assert!(
            api_key.is_empty(),
            "Remote provider should carry empty key for caller to filter"
        );
        assert_eq!(
            *protocol,
            LLMProtocol::OpenAI,
            "Remote provider should carry OpenAI protocol (not Local)"
        );
    }

    // ─── Test 13: requires_api_key 守卫逻辑 ─────────────────────────────────

    #[test]
    fn test_requires_api_key_local_empty() {
        use kingdee_kb_lib::services::image_processor::ImageProcessor;

        // Local 协议空 key → 不需要 key
        let mut p = ImageProcessor::new(
            String::new(),
            "http://localhost:11434/v1".into(),
            "llava:latest".into(),
        );
        p.set_protocol(LLMProtocol::Local);
        assert!(!p.requires_api_key(), "Local 协议空 key 不应要求 API 密钥");
    }

    #[test]
    fn test_requires_api_key_remote_empty() {
        use kingdee_kb_lib::services::image_processor::ImageProcessor;

        // OpenAI 协议空 key → 需要 key（守卫应拦截）
        let mut p = ImageProcessor::new(
            String::new(),
            "https://api.openai.com/v1".into(),
            "gpt-4o".into(),
        );
        p.set_protocol(LLMProtocol::OpenAI);
        assert!(p.requires_api_key(), "OpenAI 协议空 key 应要求 API 密钥");
    }

    #[test]
    fn test_requires_api_key_remote_has_key() {
        use kingdee_kb_lib::services::image_processor::ImageProcessor;

        // OpenAI 协议有 key → 不需要（已提供）
        let mut p = ImageProcessor::new(
            "sk-test".into(),
            "https://api.openai.com/v1".into(),
            "gpt-4o".into(),
        );
        p.set_protocol(LLMProtocol::OpenAI);
        assert!(!p.requires_api_key(), "有 API 密钥时不应要求额外密钥");
    }

    #[test]
    fn test_requires_api_key_anthropic_empty() {
        use kingdee_kb_lib::services::image_processor::ImageProcessor;

        // Anthropic 协议空 key → 需要 key
        let mut p = ImageProcessor::new(
            String::new(),
            "https://api.anthropic.com".into(),
            "claude-sonnet-4-5".into(),
        );
        p.set_protocol(LLMProtocol::Anthropic);
        assert!(p.requires_api_key(), "Anthropic 协议空 key 应要求 API 密钥");
    }

    #[test]
    fn test_requires_api_key_no_protocol() {
        use kingdee_kb_lib::services::image_processor::ImageProcessor;

        // 未设置协议空 key → 需要 key（保守策略）
        let p = ImageProcessor::new(
            String::new(),
            "https://api.openai.com/v1".into(),
            "gpt-4o".into(),
        );
        assert!(p.requires_api_key(), "未设置协议时空 key 应要求 API 密钥");
    }

    // ─── Test: Anthropic URL 归一化 ───────────────────────────────────────────

    #[test]
    fn test_anthropic_url_with_trailing_v1() {
        // base_url 已含 /v1 → 应去掉后重新拼接
        let url = anthropic_messages_url("https://api.anthropic.com/v1");
        assert_eq!(
            url, "https://api.anthropic.com/v1/messages",
            "base_url 含 /v1 时不应产生 /v1/v1/messages"
        );
    }

    #[test]
    fn test_anthropic_url_without_v1() {
        // base_url 不含 /v1 → 直接拼接
        let url = anthropic_messages_url("https://api.anthropic.com");
        assert_eq!(
            url, "https://api.anthropic.com/v1/messages",
            "base_url 不含 /v1 时应正常拼接"
        );
    }

    #[test]
    fn test_anthropic_url_trailing_slash() {
        // 尾部斜杠 + /v1
        let url = anthropic_messages_url("https://api.anthropic.com/v1/");
        assert_eq!(
            url, "https://api.anthropic.com/v1/messages",
            "尾部斜杠 + /v1 应正确归一化"
        );
    }
}
