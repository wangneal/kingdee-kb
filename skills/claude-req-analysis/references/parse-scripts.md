# 文件解析脚本参考

## Excel 解析（xlsx/xls）

```bash
python3 -c "
import openpyxl, sys
wb = openpyxl.load_workbook(sys.argv[1], data_only=True)
for name in wb.sheetnames:
    ws = wb[name]
    print(f'=== Sheet: {name} ===')
    for row in ws.iter_rows(values_only=True):
        print('\t'.join(str(c) if c is not None else '' for c in row))
" "<文件路径>"
```

## CSV 解析

```bash
python3 -c "
import csv, sys
with open(sys.argv[1], encoding='utf-8-sig') as f:
    for row in csv.reader(f):
        print('\t'.join(row))
" "<文件路径>"
```

## Word 解析（docx）

```bash
python3 -c "
import docx, sys
doc = docx.Document(sys.argv[1])
for p in doc.paragraphs:
    if p.text.strip():
        print(p.text)
for t in doc.tables:
    for row in t.rows:
        print('\t'.join(c.text.strip() for c in row.cells))
" "<文件路径>"
```

## 依赖安装

```bash
pip install openpyxl python-docx
```
