//! usearch HNSW vector index management
//!
//! Wraps usearch Index for create/insert/search/save/load operations.
//! Uses 512-dim vectors with cosine distance, BF16 quantization.
//! Index persisted to `~/.kingdee-kb/index/vectors.usearch`.
//!
//! ## Safety: Auto-reserve before add()
//!
//! usearch C++ `add()` accesses `contexts_[config.thread]` which is only
//! allocated by `try_reserve()`. Calling `add()` on an un-reserved index
//! causes Access Violation 0xc0000005 (null pointer + offset). We prevent
//! this by auto-reserving `MIN_RESERVE_CAPACITY` before the first `add()`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use usearch::{new_index, Index, IndexOptions, MetricKind, ScalarKind};

/// Default HNSW parameters for bge-small-zh-v1.5 (512-dim)
const DEFAULT_CONNECTIVITY: usize = 16;
const DEFAULT_EXPANSION_ADD: usize = 200;
const DEFAULT_EXPANSION_SEARCH: usize = 64;

/// Minimum reserve capacity to ensure usearch `contexts_` buffer is allocated.
///
/// usearch `add()` crashes (Access Violation 0xc0000005) if `contexts_` is
/// null — which happens when `reserve()` has never been called. We auto-reserve
/// this minimum capacity before the first `add()` to prevent the crash.
const MIN_RESERVE_CAPACITY: usize = 1024;

/// A search result from the vector index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The key (vector ID) in the index
    pub key: u64,
    /// Cosine distance (0 = identical, 2 = opposite)
    pub distance: f32,
    /// Cosine similarity = 1 - distance (for cosine metric)
    pub similarity: f32,
}

/// Index statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub vector_count: usize,
    pub dimensions: usize,
    pub index_path: String,
}

/// HNSW vector index manager
pub struct VectorIndex {
    index: Index,
    index_path: PathBuf,
    dimensions: usize,
    /// Track whether contexts_ has been allocated (reserve called or index loaded with data).
    /// Prevents redundant reserve calls and the Access Violation crash.
    reserved: bool,
}

impl VectorIndex {
    /// Create a new empty index, persisting to the given directory
    /// `dimensions` should match the embedding model output (512 for BGE, 384 for MiniLM).
    pub fn new(index_dir: PathBuf) -> Result<Self, String> {
        Self::with_dimensions(index_dir, 512)
    }

    /// Create a new empty index with explicit dimensions
    pub fn with_dimensions(index_dir: PathBuf, dimensions: usize) -> Result<Self, String> {
        std::fs::create_dir_all(&index_dir)
            .map_err(|e| format!("Failed to create index directory: {}", e))?;

        let index_path = index_dir.join("vectors.usearch");

        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::BF16,
            connectivity: DEFAULT_CONNECTIVITY,
            expansion_add: DEFAULT_EXPANSION_ADD,
            expansion_search: DEFAULT_EXPANSION_SEARCH,
            multi: true,
        };

        let index = new_index(&options).map_err(|e| format!("Failed to create index: {}", e))?;

        Ok(Self {
            index,
            index_path,
            dimensions,
            reserved: false,
        })
    }

    /// Load an existing index from disk
    /// `dimensions` must match the index that was saved.
    pub fn load(index_path: PathBuf) -> Result<Self, String> {
        Self::load_with_dimensions(index_path, 512)
    }

    /// Load an existing index from disk with explicit dimensions.
    ///
    /// If the on-disk index was built with `multi: false` (legacy), it is
    /// automatically deleted and a new `multi: true` index is created in its
    /// place. The caller should re-ingest documents to repopulate the index.
    pub fn load_with_dimensions(index_path: PathBuf, dimensions: usize) -> Result<Self, String> {
        // Try to read metadata from the existing index file.
        // If it was built with multi:false, delete it — we can't load it
        // with multi:true (usearch rejects mismatched multi flag).
        if index_path.exists() {
            if let Ok(meta) = Index::metadata(
                index_path.to_str().unwrap_or(""),
            ) {
                if !meta.multi {
                    // Legacy index with multi:false — delete and recreate.
                    // The caller will need to re-ingest, but this prevents
                    // the "Duplicate keys" crash at runtime.
                    let _ = std::fs::remove_file(&index_path);
                }
            }
        }

        if !index_path.exists() {
            // No index file (deleted above or never created) — return a fresh index.
            let index_dir = index_path.parent().ok_or("Invalid index path: no parent")?;
            return Self::with_dimensions(index_dir.to_path_buf(), dimensions);
        }

        let options = IndexOptions {
            dimensions,
            metric: MetricKind::Cos,
            quantization: ScalarKind::BF16,
            connectivity: DEFAULT_CONNECTIVITY,
            expansion_add: DEFAULT_EXPANSION_ADD,
            expansion_search: DEFAULT_EXPANSION_SEARCH,
            multi: true,
        };

        let index = new_index(&options).map_err(|e| format!("Failed to create index: {}", e))?;

        let path_str = index_path
            .to_str()
            .ok_or("Invalid index path (non-UTF8)")?;

        index
            .load(path_str)
            .map_err(|e| format!("Failed to load index from {:?}: {}", index_path, e))?;

        // A loaded index with existing vectors already has contexts_ allocated
        let reserved = index.capacity() > 0 || index.size() > 0;

        Ok(Self {
            index,
            index_path: index_path.to_path_buf(),
            dimensions,
            reserved,
        })
    }

    /// Ensure the usearch `contexts_` buffer is allocated before `add()`/`search()`.
    ///
    /// Auto-reserves `MIN_RESERVE_CAPACITY` if no reserve has been performed yet.
    /// This prevents the Access Violation 0xc0000005 crash that occurs when
    /// `add()` dereferences a null `contexts_` pointer.
    fn ensure_reserved(&mut self) -> Result<(), String> {
        if !self.reserved {
            let current_cap = self.index.capacity();
            let capacity = if current_cap > 0 {
                current_cap
            } else {
                MIN_RESERVE_CAPACITY
            };
            self.index
                .reserve(capacity)
                .map_err(|e| format!("Failed to auto-reserve {}: {}", capacity, e))?;
            self.reserved = true;
        }
        Ok(())
    }

    /// Add a single vector with the given key
    pub fn add(&mut self, key: u64, vector: &[f32]) -> Result<(), String> {
        self.ensure_reserved()?;
        self.index
            .add(key, vector)
            .map_err(|e| format!("Failed to add vector {}: {}", key, e))
    }

    /// Add multiple vectors in batch
    pub fn add_batch(&mut self, keys: &[u64], vectors: &[Vec<f32>]) -> Result<(), String> {
        if keys.len() != vectors.len() {
            return Err(format!(
                "keys.len() ({}) != vectors.len() ({})",
                keys.len(),
                vectors.len()
            ));
        }

        self.ensure_reserved()?;

        for (key, vector) in keys.iter().zip(vectors.iter()) {
            self.index
                .add(*key, vector)
                .map_err(|e| format!("Failed to add vector {}: {}", key, e))?;
        }

        Ok(())
    }

    /// Search for the top_k nearest neighbors
    pub fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<SearchResult>, String> {
        let results = self
            .index
            .search(query, top_k)
            .map_err(|e| format!("Search failed: {}", e))?;

        let search_results: Vec<SearchResult> = results
            .keys
            .iter()
            .zip(results.distances.iter())
            .map(|(&key, &distance)| SearchResult {
                key,
                distance,
                similarity: 1.0 - distance,
            })
            .collect();

        Ok(search_results)
    }

    /// Remove a vector by key from the index.
    ///
    /// Returns the number of vectors removed (0 if key not found, 1 if found).
    pub fn remove(&self, key: u64) -> Result<usize, String> {
        self.index
            .remove(key)
            .map_err(|e| format!("Failed to remove key {}: {}", key, e))
    }

    /// Save the index to disk
    pub fn save(&self) -> Result<(), String> {
        let path_str = self
            .index_path
            .to_str()
            .ok_or("Invalid index path (non-UTF8)")?;

        self.index
            .save(path_str)
            .map_err(|e| format!("Failed to save index to {:?}: {}", self.index_path, e))
    }

    /// Get the number of vectors in the index
    pub fn len(&self) -> usize {
        self.index.size()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get index capacity
    pub fn capacity(&self) -> usize {
        self.index.capacity()
    }

    /// Reserve capacity for expected number of vectors
    pub fn reserve(&mut self, capacity: usize) -> Result<(), String> {
        self.index
            .reserve(capacity)
            .map_err(|e| format!("Failed to reserve {}: {}", capacity, e))?;
        self.reserved = true;
        Ok(())
    }

    /// Get index statistics
    pub fn stats(&self) -> IndexStats {
        IndexStats {
            vector_count: self.len(),
            dimensions: self.dimensions,
            index_path: self.index_path.to_string_lossy().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_vector_index_crud() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = VectorIndex::new(tmp.path().to_path_buf()).unwrap();

        assert_eq!(index.len(), 0);
        assert!(index.is_empty());

        let v0 = random_vector(512, 42);
        index.add(1, &v0).unwrap();
        assert_eq!(index.len(), 1);
        assert!(!index.is_empty());

        // Search for self
        let results = index.search(&v0, 3).unwrap();
        assert_eq!(results[0].key, 1);
        assert!(results[0].distance < 0.01);
        assert!(results[0].similarity > 0.99);

        // Remove
        let removed = index.remove(1).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(index.len(), 0); // remove() decrements size

        // Save and reload
        index.save().unwrap();
        let index_path = tmp.path().join("vectors.usearch");
        assert!(index_path.exists());

        let loaded = VectorIndex::load(index_path).unwrap();
        assert_eq!(loaded.len(), 0); // vector was removed before save
    }

    #[test]
    fn test_vector_index_batch() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = VectorIndex::new(tmp.path().to_path_buf()).unwrap();

        let vectors: Vec<Vec<f32>> = (0..10).map(|i| random_vector(512, i)).collect();
        let keys: Vec<u64> = (0..10).collect();
        index.add_batch(&keys, &vectors).unwrap();
        assert_eq!(index.len(), 10);

        let results = index.search(&vectors[0], 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].key, 0); // self should be first
    }

    #[test]
    fn test_add_without_explicit_reserve_does_not_crash() {
        // This test verifies the core fix: calling add() on a fresh index
        // (without explicit reserve) should NOT cause Access Violation 0xc0000005.
        // Previously, usearch add() would dereference null contexts_ pointer.
        let tmp = tempfile::tempdir().unwrap();
        let mut index = VectorIndex::new(tmp.path().to_path_buf()).unwrap();

        // No explicit reserve() call — auto-reserve should kick in
        let v = random_vector(512, 1);
        index.add(1, &v).unwrap(); // Should NOT crash
        assert_eq!(index.len(), 1);
    }
}