# -*- coding: utf-8 -*-
import os

file_path = r'E:\projects\kingdee\KingdeeKB\src-tauri\src\services\tool_policy.rs'

with open(file_path, 'r', encoding='utf-8') as f:
    content = f.read()

# New rule to add before the closing
new_rule = '- **知识库搜索强制规则**：回答涉及金蝶产品、项目实施、技术配置等问题时，必须先调用 search-knowledge 工具搜索知识库。搜索结果是回答的事实依据，必须引用来源。不得凭记忆或通用知识回答专业问题。\n'

# Insert before the closing "#\n}
content = content.replace('"#\n}', '"' + '\n' + new_rule + '#\n}')

with open(file_path, 'w', encoding='utf-8') as f:
    f.write(content)

print('Done!')
