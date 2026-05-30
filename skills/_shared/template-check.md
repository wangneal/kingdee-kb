# 模板检查标准流程（Template Check）

各阶段 skill 在 **Step 0 前置检查** 中必须执行模板就绪检查。模板文件明文存放于 **Gitee 私有仓库**，下载脚本 `download_templates.py` 为纯 Python，经 Gitee raw API（HTTPS + access_token）拉取后解压，Windows / macOS / Linux 通用。

## 检查命令

```bash
# 检查所有阶段模板状态
python .claude/scripts/download_templates.py <项目目录> --check
```

## 下载命令

```bash
# 下载单个阶段
python .claude/scripts/download_templates.py <项目目录> <阶段目录名>

# 下载全部
python .claude/scripts/download_templates.py <项目目录> --all
```

## 阶段目录名映射

| Skill | 阶段目录 |
|-------|---------|
| kickoff-pack | 01_启动阶段 |
| survey-assistant | 02_需求阶段 |
| blueprint-tools | 03_方案阶段 |
| build-tracker | 04_构建阶段 |
| test-manager | 05_测试阶段 |
| golive-pack | 06_上线阶段 |
| acceptance-pack | 07_验收阶段 |

## Step 0 标准流程

```
### 0.X 检查模板就绪

执行模板检查：
  python .claude/scripts/download_templates.py <项目目录> --check

结果处理：
  ✅ 已就绪 → 继续后续步骤
  ⬜ 需下载 → 提示用户后自动下载：
       「📥 {阶段名}模板（{N}个）尚未下载，正在从 Gitee 私有仓库下载...」
       python .claude/scripts/download_templates.py <项目目录> <阶段目录>
       完成后展示 ✅ 模板就绪，继续流程
  ❌ 下载失败 → 提示用户：
       「📥 模板下载失败。请确认 skill 套件安装完整（.claude/template-token 缺失或失效）及网络可用。」
       继续流程（降级为纯 Markdown 输出，不含模板填充）
```

## 托管与访问说明（两层模型）

- **访问控制层**：套件仅发布到公司 Kingdee Skill Hub，员工下载套件即获得访问令牌
- **防泄露层**：模板存放于 Gitee **私有仓库**，明文 `{阶段}_templates.zip`，不被公网索引/访问
- `.claude/template-token` 为 Gitee access token，随套件通过 Hub 分发
- 下载脚本 `download_templates.py` 纯 Python，经 Gitee raw API（HTTPS + `access_token`）下载，无需 git / ssh / OpenSSL，跨平台
