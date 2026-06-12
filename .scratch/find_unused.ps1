$unused = @("embed_batch","embed_text","force_recompile_kb_source","generate_followup_questions","get_api_key","get_available_providers","get_current_edition","get_data_dir","get_index_stats","get_investigation_recipe","get_knowledge_stats","get_product","greet","import_research_outlines","list_research_modules","load_index","process_ingestion_queue","recommend_questions","retry_failed_ingestions","scan_index_drift","scan_stale_skills","search_similar","seed_demo_wiki_pages","set_edition","smart_fill_for_question","traverse_graph")
Write-Host "=== Internal call sites (excluding handler/definition) ==="
foreach ($cmd in $unused) {
  Write-Host "--- $cmd ---"
  $files = Get-ChildItem -Path "src-tauri\src" -Recurse -Include *.rs
  foreach ($f in $files) {
    if ($f.FullName -like "*\lib.rs") { continue }
    $matches = Select-String -Path $f.FullName -Pattern ("\b" + $cmd + "\b")
    foreach ($m in $matches) {
      $rel = $f.FullName.Replace((Get-Location).Path + "\src-tauri\src\", "")
      $line = $m.Line.Trim()
      if ($line.Length -gt 120) { $line = $line.Substring(0,120) + "..." }
      Write-Host ($rel + ":" + $m.LineNumber + " " + $line)
    }
  }
}
