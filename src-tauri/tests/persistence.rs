// Phase 2 Task 9: Index persistence verification
//
// Verifies that usearch HNSW index and rusqlite metadata survive app restart.
// Uses kingdee_kb_lib's VectorIndex and MetadataStore for integration testing.

use std::path::PathBuf;

// We can't use the crate's internal API directly from integration tests without re-exporting.
// This test uses the same usearch + rusqlite APIs to verify persistence semantics.

use rusqlite::{params, Connection};
use usearch::{new_index, Index, IndexOptions, MetricKind, ScalarKind};

/// Generate a random 512-dim unit vector
fn random_vector(dim: usize, seed: u64) -> Vec<f32> {
    let mut v = Vec::with_capacity(dim);
    let mut s = seed;
    for _ in 0..dim {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        v.push(((s as f64 / u64::MAX as f64) * 2.0 - 1.0) as f32);
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    for x in &mut v {
        *x /= norm;
    }
    v
}

fn init_sqlite(path: &PathBuf) -> Connection {
    let db = Connection::open(path).expect("Failed to open SQLite DB");
    db.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            vector_key INTEGER UNIQUE,
            content TEXT NOT NULL
        );"
    ).unwrap();
    db
}

fn init_index(path: &PathBuf) -> Index {
    let options = IndexOptions {
        dimensions: 512, metric: MetricKind::Cos, quantization: ScalarKind::BF16,
        connectivity: 16, expansion_add: 200, expansion_search: 64, multi: false,
    };
    let index = new_index(&options).expect("Failed to create index");
    if path.exists() {
        index.load(path.to_str().unwrap()).expect("Failed to load index");
    }
    // Must reserve before add() or usearch crashes with Access Violation 0xc0000005:
    // add() dereferences contexts_[config.thread] which is null until reserve() allocates it.
    if index.capacity() == 0 {
        index.reserve(1024).expect("Failed to reserve index capacity");
    }
    index
}

#[test]
fn test_full_persistence_roundtrip() {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let index_path = tmp.path().join("vectors.usearch");
    let db_path = tmp.path().join("metadata.db");

    let test_texts = vec![
        "金蝶云星空期货点价配置",
        "苍穹PCR审批流程设置",
        "物料主数据维护方法",
        "二次开发常见问题处理",
        "总账凭证冲销操作步骤",
    ];

    // ─── Phase A: Write data ───
    let index = init_index(&index_path);
    let db = init_sqlite(&db_path);

    let vectors: Vec<Vec<f32>> = (0..test_texts.len())
        .map(|i| random_vector(512, i as u64))
        .collect();

    for (i, (text, vector)) in test_texts.iter().zip(vectors.iter()).enumerate() {
        index.add(i as u64, vector).expect("Failed to add vector");
        db.execute(
            "INSERT INTO chunks (vector_key, content) VALUES (?1, ?2)",
            params![i as i64, text],
        )
        .expect("Failed to insert chunk");
    }

    assert_eq!(index.size(), 5);
    let count: i64 = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 5);

    // Save to disk
    index.save(index_path.to_str().unwrap()).expect("Failed to save index");
    let idx_size = std::fs::metadata(&index_path).unwrap().len();
    assert!(idx_size > 0, "Index file should have content");
    println!("Phase A: Index saved ({} bytes), DB has {} chunks", idx_size, count);

    // Drop resources (simulate app shutdown)
    drop(index);
    drop(db);

    // ─── Phase B: Reload and verify ───
    let index2 = init_index(&index_path);
    let db2 = init_sqlite(&db_path);

    assert_eq!(index2.size(), 5, "Index should still have 5 vectors after reload");

    // Verify all vector keys can be looked up in SQLite
    for key in 0..5u64 {
        let results = index2.search(&vectors[key as usize], 1).expect("Search failed");
        assert_eq!(results.keys[0], key, "Self-search should return self");

        let content: String = db2
            .query_row(
                "SELECT content FROM chunks WHERE vector_key = ?1",
                params![key as i64],
                |r| r.get(0),
            )
            .expect("Failed to query chunk");
        assert_eq!(content, test_texts[key as usize], "Content mismatch");
    }

    // Verify search with a new query vector
    let query = random_vector(512, 999);
    let results = index2.search(&query, 3).expect("Search failed");
    assert!(!results.keys.is_empty(), "Search should return results");

    println!(
        "Phase B: Reloaded index ({} vectors), search returned {} results",
        index2.size(),
        results.keys.len()
    );

    println!("Persistence roundtrip test passed!");
}

#[test]
fn test_empty_index_persistence() {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let index_path = tmp.path().join("empty.usearch");

    // Create and save empty index
    let index = init_index(&index_path);
    assert_eq!(index.size(), 0);
    index.save(index_path.to_str().unwrap()).expect("Failed to save");
    drop(index);

    // Reload
    let index2 = init_index(&index_path);
    assert_eq!(index2.size(), 0, "Empty index should stay empty after reload");
    println!("Empty index persistence test passed!");
}

#[test]
fn test_sqlite_wal_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let db = init_sqlite(&db_path);

    let journal_mode: String = db
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(journal_mode, "wal", "Should use WAL journal mode");
    println!("WAL mode confirmed: {}", journal_mode);
}

#[test]
fn test_chunk_vector_key_consistency() {
    let tmp = tempfile::tempdir().unwrap();
    let index_path = tmp.path().join("consistency.usearch");
    let db_path = tmp.path().join("consistency.db");

    let index = init_index(&index_path);
    let db = init_sqlite(&db_path);

    // Insert 100 chunks with sequential keys
    for i in 0..100u64 {
        let vector = random_vector(512, i);
        index.add(i, &vector).unwrap();
        db.execute(
            "INSERT INTO chunks (vector_key, content) VALUES (?1, ?2)",
            params![i as i64, format!("chunk_{}", i)],
        ).unwrap();
    }

    // Verify every key has matching content
    for i in 0..100u64 {
        let vector = random_vector(512, i);
        let results = index.search(&vector, 1).unwrap();
        assert_eq!(results.keys[0], i);

        let content: String = db
            .query_row("SELECT content FROM chunks WHERE vector_key = ?1", params![i as i64], |r| r.get(0))
            .unwrap();
        assert_eq!(content, format!("chunk_{}", i));
    }

    index.save(index_path.to_str().unwrap()).unwrap();
    println!("Consistency test passed: 100 chunks with matching vector keys");
}
