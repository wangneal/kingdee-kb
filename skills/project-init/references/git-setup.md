# Git/Gitee 团队协作配置

## 检查 Git 环境

执行 `git --version` 检查 Git 是否已安装。

- **Git 未安装** → 提示：「⚠️ 未检测到 Git，请先安装 Git 后再配置。macOS 可通过 `xcode-select --install` 安装，Windows 可从 https://git-scm.com 下载。安装后运行 /project-sync setup 完成配置。」
- **Git 已安装** → 继续下一步。

## 获取 Gitee 仓库地址

```
📋 请提供 Gitee 仓库地址（如：https://gitee.com/username/project.git）

如果还没有创建仓库：
1. 打开 https://gitee.com 并登录
2. 点击右上角「+」→「新建仓库」
3. 仓库名称建议使用项目名称
4. 设置为「私有」仓库
5. 创建后复制仓库的 HTTPS 地址粘贴到这里

输入仓库地址（或输入"跳过"稍后配置）：
```

- 用户提供 URL → 验证是否为 `https://gitee.com/` 或 `git@gitee.com:` 开头的合法地址
- 用户输入"跳过" → 进入 Step 7
- URL 格式无效 → 追问：「格式似乎不正确，请提供完整的 Gitee 仓库地址，例如 https://gitee.com/kingdee/myproject.git」

## 初始化 Git 仓库

按顺序执行以下操作，每步检查结果：

### a. 初始化仓库
```
git init
```
- 失败 → 提示错误信息，跳过后续步骤，进入 Step 7

### b. 创建 .gitignore 文件

在项目根目录创建 `.gitignore`：

```
# Office temp files
~$*.docx
~$*.xlsx
~$*.pptx
~$*.doc
~$*.xls
~$*.ppt
*.tmp

# macOS
.DS_Store
.AppleDouble
.LSOverride
._*

# Windows
Thumbs.db
ehthumbs.db
Desktop.ini
```

### c. 首次提交
```
git add -A
git commit -m "项目启动：全量交付物模板"
```
- 如果 `git add -A` 警告大文件（如 .pptx），这是正常的，继续执行
- 如果 commit 失败（如无文件可提交），提示并继续

### d. 关联远程仓库
```
git remote add origin <url>
```
- 如果 remote origin 已存在 → 先执行 `git remote remove origin` 再重新添加
- 失败 → 提示错误，继续

### e. 推送到远程
```
git push -u origin master
```
- **成功** → 进入结果展示
- **失败**的常见原因与处理：
  - 远程仓库为空但尚未创建 → 提示：「请确认 Gitee 仓库已创建成功。仓库需存在于 Gitee 上才能推送。」
  - 认证失败 → 提示：「⚠️ 认证失败。请确认：1) Gitee 账号密码正确；2) 如使用 HTTPS，确保仓库地址无误。可尝试在终端中先执行 `git config --global credential.helper osxkeychain`（macOS）后再推送。」
  - 远程已有内容 → 提示：「远程仓库已有内容。如需保留远程内容请运行 /project-sync pull，如需覆盖请手动执行 `git push -u origin master --force`。注意：强制推送会覆盖远程已有文件。」
  - `master` 分支不存在（新版 Git 默认分支为 `main`）→ 检查当前分支：`git branch --show-current`，使用实际分支名推送
  - **无论成功与否，不中断流程**

## 更新 CLAUDE.md 配置

如果推送成功，在 `CLAUDE.md` 的 `## 项目配置` 表格中追加一行：

```
| Git远程仓库 | {url} |
```

如果推送失败，追加：

```
| Git远程仓库 | {url}（待推送） |
```

## 结果展示

**成功时：**
```
✅ Git/Gitee 团队协作配置完成！

📋 配置摘要：
- 本地仓库：{项目目录}
- 远程仓库：{url}
- 分支：{实际分支名}
- 已提交：全量交付物模板

团队成员可通过以下命令参与协作：
- /project-sync pull  — 拉取最新文件
- /project-sync push  — 保存并同步文件
- /project-sync status — 查看同步状态
```

**部分成功时（推送失败）：**
```
⚠️ Git 仓库已初始化但推送未成功

已完成：
✅ Git 仓库初始化
✅ .gitignore 创建
✅ 首次提交完成
❌ 推送到 {url} 失败：{错误信息}

本地文件已保存到 Git 历史中。请检查以上错误后，运行 /project-sync push 重试推送。
```
