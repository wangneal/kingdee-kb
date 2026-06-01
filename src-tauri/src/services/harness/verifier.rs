//! 结果验证 + 重试上限
//!
//! 验证 Agent 步骤执行结果，检测失败模式，
//! 在连续失败达到上限时触发重新规划或终止执行。

/// 验证器状态
#[derive(Debug, Clone, PartialEq)]
pub enum VerificationStatus {
    /// 验证通过
    Pass,
    /// 验证失败，可以重试
    Fail(String),
    /// 连续失败次数耗尽
    Exhausted(String),
    /// 需要重新规划
    NeedsReplan(String),
}

/// 结果验证器
pub struct ResultVerifier {
    /// 最大连续失败次数
    max_consecutive_failures: usize,
    /// 当前连续失败计数
    consecutive_failures: usize,
    /// 步骤总失败数
    total_failures: usize,
}

impl ResultVerifier {
    pub fn new(max_consecutive_failures: usize) -> Self {
        Self {
            max_consecutive_failures,
            consecutive_failures: 0,
            total_failures: 0,
        }
    }

    /// 验证步骤执行结果
    pub fn verify(
        &mut self,
        _step_description: &str,
        result: &str,
        expected_output: &str,
    ) -> VerificationStatus {
        // 检查结果是否为空
        if result.trim().is_empty() {
            return self.record_failure("步骤结果为空");
        }

        // 检查是否包含失败关键词
        let failure_keywords = [
            "失败", "错误", "error", "failed", "exception",
            "超时", "timeout", "不支持", "not supported",
        ];
        let result_lower = result.to_lowercase();
        for keyword in &failure_keywords {
            if result_lower.contains(keyword) {
                return self.record_failure(&format!("结果包含失败关键词: '{}'", keyword));
            }
        }

        // 检查结果长度是否异常短（可能是不完整的输出）
        if result.len() < 10 && expected_output.len() > 50 {
            return self.record_failure("结果过短，可能是不完整的输出");
        }

        // 验证通过
        self.consecutive_failures = 0;
        VerificationStatus::Pass
    }

    /// 记录失败，检查是否耗尽重试次数
    fn record_failure(&mut self, reason: &str) -> VerificationStatus {
        self.consecutive_failures += 1;
        self.total_failures += 1;

        if self.consecutive_failures >= self.max_consecutive_failures {
            tracing::warn!(
                consecutive = self.consecutive_failures,
                max = self.max_consecutive_failures,
                "连续失败次数达到上限"
            );
            return VerificationStatus::NeedsReplan(format!(
                "连续失败 {} 次: {}", self.consecutive_failures, reason
            ));
        }

        VerificationStatus::Fail(reason.to_string())
    }

    /// 重置连续失败计数（步骤成功后调用）
    pub fn reset_consecutive(&mut self) {
        self.consecutive_failures = 0;
    }

    pub fn consecutive_failures(&self) -> usize {
        self.consecutive_failures
    }

    pub fn total_failures(&self) -> usize {
        self.total_failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_pass() {
        let mut verifier = ResultVerifier::new(3);
        let status = verifier.verify("搜索知识库", "找到 5 条相关结果", "文档列表");
        assert_eq!(status, VerificationStatus::Pass);
    }

    #[test]
    fn test_verify_fail_on_error_keyword() {
        let mut verifier = ResultVerifier::new(3);
        let status = verifier.verify("搜索知识库", "搜索失败：连接超时", "文档列表");
        assert!(matches!(status, VerificationStatus::Fail(_)));
    }

    #[test]
    fn test_verify_exhausted_after_max_failures() {
        let mut verifier = ResultVerifier::new(3);
        let _ = verifier.verify("步骤1", "失败", "预期输出");
        let _ = verifier.verify("步骤1", "错误", "预期输出");
        let status = verifier.verify("步骤1", "失败", "预期输出");
        assert!(matches!(status, VerificationStatus::NeedsReplan(_)));
    }

    #[test]
    fn test_verify_reset_on_success() {
        let mut verifier = ResultVerifier::new(3);
        let _ = verifier.verify("步骤1", "失败", "预期输出");
        let _ = verifier.verify("步骤1", "失败", "预期输出");
        // 成功一次
        let _ = verifier.verify("步骤1", "成功，返回结果", "预期输出");
        assert_eq!(verifier.consecutive_failures(), 0);
        // 再失败不会触发 Exhausted（因为计数器已重置）
        let status = verifier.verify("步骤1", "失败", "预期输出");
        assert!(matches!(status, VerificationStatus::Fail(_)));
    }

    #[test]
    fn test_verify_empty_result() {
        let mut verifier = ResultVerifier::new(3);
        let status = verifier.verify("步骤1", "  ", "预期输出");
        assert!(matches!(status, VerificationStatus::Fail(_)));
    }
}
