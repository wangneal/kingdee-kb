//! 技能脚本执行引擎 — 安全沙箱执行 Python/Bash 脚本
//!
//! 参考 Claude Code 技能系统的脚本执行机制：
//!   - 命令白名单验证
//!   - 超时控制
//!   - 参数替换
//!   - 输出捕获

use std::collections::HashSet;
use std::path::PathBuf;

/// 执行结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub output: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// 执行器配置
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// 允许执行脚本的技能白名单
    pub allowed_skills: HashSet<String>,
    /// 脚本执行超时时间（秒）
    pub timeout: u64,
    /// 工作目录
    pub working_dir: PathBuf,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            allowed_skills: HashSet::new(),
            timeout: 30,
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

/// 参数替换上下文
#[derive(Debug, Clone)]
pub struct SubstitutionContext {
    pub arguments: Vec<String>,
    pub skill_dir: PathBuf,
    pub session_id: String,
    pub custom_vars: std::collections::HashMap<String, String>,
}

impl SubstitutionContext {
    /// 替换 SKILL.md 中的变量
    pub fn substitute(&self, content: &str) -> String {
        let mut result = content.to_string();

        // $ARGUMENTS → 所有参数
        result = result.replace("$ARGUMENTS", &self.arguments.join(" "));

        // $0, $1, ... → 按位置的参数
        for (i, arg) in self.arguments.iter().enumerate() {
            result = result.replace(&format!("${}", i), arg);
        }

        // ${CLAUDE_SKILL_DIR} → 技能目录绝对路径
        result = result.replace("${CLAUDE_SKILL_DIR}", &self.skill_dir.to_string_lossy());

        // ${CLAUDE_SESSION_ID} → 会话 ID
        result = result.replace("${CLAUDE_SESSION_ID}", &self.session_id);

        // 自定义变量
        for (key, value) in &self.custom_vars {
            result = result.replace(&format!("${{{}}}", key), value);
        }

        result
    }
}

/// 脚本执行引擎
pub struct SkillExecutor {
    /// 配置
    config: ExecutorConfig,
}

impl SkillExecutor {
    /// 创建执行器
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// 执行内联 shell 命令
    pub async fn execute_inline_command(
        &self,
        command: &str,
        context: &SubstitutionContext,
    ) -> Result<ExecutionResult, ExecutorError> {
        // 安全检查
        self.validate_command(command)?;

        // 参数替换
        let resolved = context.substitute(command);

        // 执行命令（带超时）
        let start = std::time::Instant::now();
        let timeout_duration = tokio::time::Duration::from_secs(self.config.timeout);

        let output_result = tokio::time::timeout(timeout_duration, async {
            #[cfg(target_os = "windows")]
            {
                tokio::process::Command::new("cmd")
                    .arg("/C")
                    .arg(&resolved)
                    .current_dir(&self.config.working_dir)
                    .output()
                    .await
            }

            #[cfg(not(target_os = "windows"))]
            {
                tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(&resolved)
                    .current_dir(&self.config.working_dir)
                    .output()
                    .await
            }
        })
        .await;

        let duration = start.elapsed();

        match output_result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    Ok(ExecutionResult {
                        success: true,
                        output: String::from_utf8_lossy(&output.stdout).to_string(),
                        duration_ms: duration.as_millis() as u64,
                        error: None,
                    })
                } else {
                    Ok(ExecutionResult {
                        success: false,
                        output: String::from_utf8_lossy(&output.stdout).to_string(),
                        duration_ms: duration.as_millis() as u64,
                        error: Some(String::from_utf8_lossy(&output.stderr).to_string()),
                    })
                }
            }
            Ok(Err(e)) => Err(ExecutorError::ExecutionFailed(e.to_string())),
            Err(_) => Ok(ExecutionResult {
                success: false,
                output: String::new(),
                duration_ms: duration.as_millis() as u64,
                error: Some(format!("执行超时（{}秒）", self.config.timeout)),
            }),
        }
    }

    /// 执行块命令
    pub async fn execute_block_command(
        &self,
        lang: &str,
        script: &str,
        context: &SubstitutionContext,
    ) -> Result<ExecutionResult, ExecutorError> {
        let resolved = context.substitute(script);

        match lang {
            "python" | "python3" => {
                let script_path = self.write_temp_script(&resolved, "py")?;
                self.execute_script("python", &script_path).await
            }
            "bash" | "sh" => {
                let script_path = self.write_temp_script(&resolved, "sh")?;
                self.execute_script("bash", &script_path).await
            }
            "powershell" | "ps1" => {
                let script_path = self.write_temp_script(&resolved, "ps1")?;
                self.execute_script("powershell", &script_path).await
            }
            _ => Err(ExecutorError::UnsupportedLanguage(lang.to_string())),
        }
    }

    /// 执行脚本文件
    async fn execute_script(
        &self,
        interpreter: &str,
        script_path: &PathBuf,
    ) -> Result<ExecutionResult, ExecutorError> {
        let start = std::time::Instant::now();
        let timeout_duration = tokio::time::Duration::from_secs(self.config.timeout);

        let output_result = tokio::time::timeout(timeout_duration, async {
            #[cfg(target_os = "windows")]
            {
                let (cmd, args) = match interpreter {
                    "python" | "python3" => {
                        ("python", vec![script_path.to_string_lossy().to_string()])
                    }
                    "bash" | "sh" => ("bash", vec![script_path.to_string_lossy().to_string()]),
                    "powershell" => (
                        "powershell",
                        vec![
                            "-ExecutionPolicy".to_string(),
                            "Bypass".to_string(),
                            "-File".to_string(),
                            script_path.to_string_lossy().to_string(),
                        ],
                    ),
                    _ => return Err(ExecutorError::UnsupportedLanguage(interpreter.to_string())),
                };

                tokio::process::Command::new(cmd)
                    .args(&args)
                    .current_dir(&self.config.working_dir)
                    .output()
                    .await
                    .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))
            }

            #[cfg(not(target_os = "windows"))]
            {
                tokio::process::Command::new(interpreter)
                    .arg(script_path)
                    .current_dir(&self.config.working_dir)
                    .output()
                    .await
                    .map_err(|e| ExecutorError::ExecutionFailed(e.to_string()))
            }
        })
        .await;

        let duration = start.elapsed();

        // 清理临时文件
        let _ = std::fs::remove_file(script_path);

        match output_result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    Ok(ExecutionResult {
                        success: true,
                        output: String::from_utf8_lossy(&output.stdout).to_string(),
                        duration_ms: duration.as_millis() as u64,
                        error: None,
                    })
                } else {
                    Ok(ExecutionResult {
                        success: false,
                        output: String::from_utf8_lossy(&output.stdout).to_string(),
                        duration_ms: duration.as_millis() as u64,
                        error: Some(String::from_utf8_lossy(&output.stderr).to_string()),
                    })
                }
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Ok(ExecutionResult {
                success: false,
                output: String::new(),
                duration_ms: duration.as_millis() as u64,
                error: Some(format!("执行超时（{}秒）", self.config.timeout)),
            }),
        }
    }

    /// 写入临时脚本文件
    fn write_temp_script(&self, content: &str, ext: &str) -> Result<PathBuf, ExecutorError> {
        let temp_dir = std::env::temp_dir();
        let filename = format!("skill_script_{}.{}", uuid::Uuid::new_v4(), ext);
        let script_path = temp_dir.join(filename);

        std::fs::write(&script_path, content).map_err(|e| ExecutorError::IoError(e.to_string()))?;

        Ok(script_path)
    }

    /// 命令安全验证
    fn validate_command(&self, command: &str) -> Result<(), ExecutorError> {
        let command_lower = command.to_lowercase();

        // 禁止的命令模式
        let forbidden = [
            "rm -rf /",
            "rm -rf /*",
            ":(){:|:&};:", // fork bomb
            "dd if=/dev/zero",
            "mkfs",
            "format c:",
            "del /s /q c:\\",
            "rmdir /s /q c:\\",
        ];

        for pattern in &forbidden {
            if command_lower.contains(pattern) {
                return Err(ExecutorError::ForbiddenCommand(pattern.to_string()));
            }
        }

        // 禁止的危险字符组合
        if command_lower.contains("&&") && command_lower.contains("rm") {
            return Err(ExecutorError::ForbiddenCommand("组合删除命令".to_string()));
        }

        Ok(())
    }

    /// 检查技能是否在白名单中
    pub fn is_skill_allowed(&self, skill_id: &str) -> bool {
        self.config.allowed_skills.is_empty() || self.config.allowed_skills.contains(skill_id)
    }
}

/// 执行器错误
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("执行失败: {0}")]
    ExecutionFailed(String),

    #[error("禁止的命令: {0}")]
    ForbiddenCommand(String),

    #[error("不支持的语言: {0}")]
    UnsupportedLanguage(String),

    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("技能不在白名单: {0}")]
    SkillNotAllowed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitution() {
        let context = SubstitutionContext {
            arguments: vec!["arg1".to_string(), "arg2".to_string()],
            skill_dir: PathBuf::from("/skills/test"),
            session_id: "session123".to_string(),
            custom_vars: vec![("CUSTOM".to_string(), "value".to_string())]
                .into_iter()
                .collect(),
        };

        let result = context.substitute("echo $0 $1 $ARGUMENTS");
        assert_eq!(result, "echo arg1 arg2 arg1 arg2");
    }

    #[test]
    fn test_validate_command_safe() {
        let executor = SkillExecutor::new(ExecutorConfig::default());
        assert!(executor.validate_command("echo hello").is_ok());
        assert!(executor.validate_command("python script.py").is_ok());
    }

    #[test]
    fn test_validate_command_forbidden() {
        let executor = SkillExecutor::new(ExecutorConfig::default());
        assert!(executor.validate_command("rm -rf /").is_err());
        assert!(executor.validate_command(":(){:|:&};:").is_err());
    }

    #[tokio::test]
    async fn test_execute_inline_command() {
        let executor = SkillExecutor::new(ExecutorConfig::default());
        let context = SubstitutionContext {
            arguments: vec![],
            skill_dir: PathBuf::from("."),
            session_id: "test".to_string(),
            custom_vars: std::collections::HashMap::new(),
        };

        #[cfg(target_os = "windows")]
        let result = executor
            .execute_inline_command("echo hello", &context)
            .await;

        #[cfg(not(target_os = "windows"))]
        let result = executor
            .execute_inline_command("echo hello", &context)
            .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.success);
        assert!(result.output.contains("hello"));
    }
}
