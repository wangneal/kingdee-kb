import os

path = r'E:\projects\kingdee\KingdeeKB\src-tauri\src\services\hybrid_search.rs'
with open(path, 'r', encoding='utf-8') as f:
    content = f.read()

old_head = '            chunks\n                .into_iter()\n                .filter_map(|c| {\n                    let (title, project) = meta\n                        .get_document(c.document_id)\n                        .ok()\n                        .flatten()\n                        .map(|d| (d.title, d.project))\n                        .unwrap_or_else(|| (String::new(), "default".to_string()));\n\n                    if let Some(pid) = project_id {\n                        if project != pid {\n                            return None;\n                        }\n                    }\n\n                    Some(ResolvedChunk {\n                        chunk_id: c.id,\n                        title,\n                        content: c.content,\n                        document_id: c.document_id,\n                        section_path: c.section_path,\n                        project,\n                    })\n                })\n                .collect()'

new_head = '            // Batch-fetch all documents (eliminates N+1 query)\n                    let doc_ids: Vec<i64> = chunks.iter().map(|c| c.document_id).collect();\n                    let doc_map = meta.get_documents_by_ids(&doc_ids)?;\n\n                    chunks\n                        .into_iter()\n                        .filter_map(|c| {\n                            let (title, project) = doc_map\n                                .get(&c.document_id)\n                                .map(|d| (d.title.clone(), d.project.clone()))\n                                .unwrap_or_else(|| (String::new(), "default".to_string()));\n\n                    if let Some(pid) = project_id {\n                        if project != pid {\n                            return None;\n                        }\n                    }\n\n                    Some(ResolvedChunk {\n                        chunk_id: c.id,\n                        title,\n                        content: c.content,\n                        document_id: c.document_id,\n                        section_path: c.section_path,\n                        project,\n                    })\n                })\n                .collect()'

if old_head in content:
    content = content.replace(old_head, new_head)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(content)
    print('SUCCESS: hybrid_search.rs N+1 query fixed')
else:
    print('WARNING: Old pattern not found')
    idx = content.find('chunks')
    if idx >= 0:
        print('Found "chunks" at index', idx)
        print(repr(content[idx:idx+300]))
    else:
        print('Could not find "chunks" in file')
