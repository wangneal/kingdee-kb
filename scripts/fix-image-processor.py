# -*- coding: utf-8 -*-
with open(r"E:\projects\kingdee\KingdeeKB\src-tauri\src\services\image_processor.rs", "r", encoding="utf-8") as f:
    content = f.read()

content = content.replace(
    ".as_array().unwrap_or_default()",
    ".as_array().map_or(&[][..], |v| v)"
)

with open(r"E:\projects\kingdee\KingdeeKB\src-tauri\src\services\image_processor.rs", "w", encoding="utf-8") as f:
    f.write(content)

print("Fixed")
