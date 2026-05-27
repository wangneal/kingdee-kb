// Phase 2 SPIKE: Validate usearch HNSW + embedding model on Windows
//
// Part A: usearch HNSW create/insert/save/load/search (random vectors, no model)
// Part B: fastembed-rs embedding model (deferred - requires HuggingFace access or pre-bundled model)
//
// NOTE: HuggingFace is blocked in China. The bge-small-zh-v1.5 ONNX model (~48MB)
// cannot be downloaded in the current network environment. Solutions:
//   1. Pre-bundle the ONNX model with the app
//   2. Use a Chinese CDN mirror for model hosting
//   3. Set up HTTP_PROXY to access HuggingFace
// See Task 3 (ModelManager) for resolution.

use usearch::{new_index, Index, IndexOptions, MetricKind, ScalarKind};

/// Compute cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Generate a random 512-dim unit vector (simulating bge-small-zh-v1.5 embedding)
fn random_vector(dim: usize, seed: u64) -> Vec<f32> {
    let mut v = Vec::with_capacity(dim);
    let mut s = seed;
    for _ in 0..dim {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        v.push(((s as f64 / u64::MAX as f64) * 2.0 - 1.0) as f32);
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    for x in &mut v {
        *x /= norm;
    }
    v
}

#[test]
fn spike_usearch_hnsw_roundtrip() {
    println!("[SPIKE] Testing usearch HNSW with 512-dim vectors...");

    // ─── 1. Create usearch HNSW index ───
    let options = IndexOptions {
        dimensions: 512,
        metric: MetricKind::Cos,
        quantization: ScalarKind::BF16,
        connectivity: 16,
        expansion_add: 200,
        expansion_search: 64,
        multi: false,
    };
    let index: Index = new_index(&options).expect("Failed to create index");
    assert_eq!(index.size(), 0);
    assert_eq!(index.dimensions(), 512);
    println!(
        "[SPIKE] Index created: dims={}, conn={}",
        index.dimensions(),
        index.connectivity()
    );

    // ─── 2. Reserve and add vectors ───
    index.reserve(10).expect("Failed to reserve");
    let v0 = random_vector(512, 0);
    let v1 = random_vector(512, 1);
    let v2 = random_vector(512, 2);
    let mut v3 = random_vector(512, 0);
    v3[0] += 0.001; // near v0

    index.add(100, &v0).expect("Failed to add v0");
    index.add(101, &v1).expect("Failed to add v1");
    index.add(102, &v2).expect("Failed to add v2");
    index.add(103, &v3).expect("Failed to add v3");
    assert_eq!(index.size(), 4);
    println!("[SPIKE] Added 4 vectors, size: {}", index.size());

    // ─── 3. Search ───
    let results = index.search(&v0, 3).expect("Search failed");
    assert!(!results.keys.is_empty());
    assert_eq!(results.keys[0], 100, "Self should be first");
    println!(
        "[SPIKE] Search: keys={:?}, distances={:?}",
        results.keys, results.distances
    );
    assert!(
        results.distances[0] < 0.01,
        "Self cos distance should be near 0"
    );

    // ─── 4. Persist to temp file ───
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let index_path = tmp.path().join("test_spike.usearch");
    let path_str = index_path.to_str().expect("Invalid path");
    index.save(path_str).expect("Failed to save index");
    let file_size = std::fs::metadata(&index_path).unwrap().len();
    println!("[SPIKE] Index saved: {} bytes", file_size);

    // ─── 5. Reload from disk ───
    let options2 = IndexOptions {
        dimensions: 512,
        metric: MetricKind::Cos,
        quantization: ScalarKind::BF16,
        connectivity: 16,
        expansion_add: 200,
        expansion_search: 64,
        multi: false,
    };
    let index2: Index = new_index(&options2).expect("Failed to create index2");
    index2.load(path_str).expect("Failed to load index");
    assert_eq!(index2.size(), 4, "Loaded index size mismatch");
    println!("[SPIKE] Index reloaded, size: {}", index2.size());

    // Verify search after reload
    let results2 = index2
        .search(&v0, 3)
        .expect("Search on loaded index failed");
    assert_eq!(results2.keys[0], 100);
    for i in 0..results.keys.len() {
        assert_eq!(results2.keys[i], results.keys[i], "Key mismatch at {}", i);
    }
    println!("[SPIKE] Reload search: keys={:?}", results2.keys);

    // ─── 6. Test remove ───
    index2.remove(101).expect("Failed to remove key 101");
    assert_eq!(index2.size(), 3);
    let results3 = index2.search(&v0, 4).expect("Search after remove");
    assert!(
        !results3.keys.iter().any(|&k| k == 101),
        "Removed key should not appear"
    );
    println!("[SPIKE] After remove (size=3): keys={:?}", results3.keys);

    println!("[SPIKE] All assertions passed!");
}

#[test]
fn spike_cosine_similarity() {
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![1.0, 0.0, 0.0];
    let c = vec![0.0, 1.0, 0.0];
    let d = vec![-1.0, 0.0, 0.0];
    assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001, "Identical");
    assert!(
        (cosine_similarity(&a, &c) - 0.0).abs() < 0.001,
        "Orthogonal"
    );
    assert!((cosine_similarity(&a, &d) + 1.0).abs() < 0.001, "Opposite");
    let v1 = random_vector(512, 42);
    let v2 = random_vector(512, 42);
    assert!(
        (cosine_similarity(&v1, &v2) - 1.0).abs() < 0.001,
        "Same seed"
    );
    let v3 = random_vector(512, 99);
    let sim = cosine_similarity(&v1, &v3);
    assert!(sim < 1.0 && sim > -1.0, "Random vectors sim in range");
    println!(
        "[SPIKE] Cosine similarity tests passed! v1-v3 sim: {:.4}",
        sim
    );
}
