// Phase 2 Task 8: Semantic similarity verification (cosine ≥ 0.7)
//
// Tests that bge-small-zh-v1.5 produces cosine similarity ≥ 0.7
// for semantically related Chinese ERP text pairs.
//
// NOTE: Requires model download from HuggingFace. If blocked, set:
//   HF_ENDPOINT=https://hf-mirror.com
// Or pre-download: python -c "from huggingface_hub import snapshot_download;
//   snapshot_download('BAAI/bge-small-zh-v1.5')"

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

fn init_model() -> TextEmbedding {
    TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::BGESmallZHV15).with_show_download_progress(true),
    )
    .expect("Failed to initialize bge-small-zh-v1.5 model")
}

fn compute_similarity(model: &mut TextEmbedding, a: &str, b: &str) -> f32 {
    let embeddings = model.embed(vec![a, b], None).expect("Embedding failed");
    cosine_similarity(&embeddings[0], &embeddings[1])
}

#[test]
#[ignore = "Requires bge-small-zh-v1.5 model download from HuggingFace"]
fn test_chinese_semantic_similarity() {
    let mut model = init_model();

    // Semantically related pairs (should have cosine similarity ≥ 0.7)
    let similar_pairs = vec![
        ("金蝶云星空如何配置期货点价", "金蝶苍穹期货点价模块设置"),
        ("客户要做二开怎么处理", "客户需要二次开发该如何操作"),
        ("PCR审批流程配置", "PCR审批流程设置方法"),
        ("物料主数据维护", "物料基础信息管理"),
    ];

    // Semantically unrelated pairs (should have cosine similarity < 0.5)
    let different_pairs = vec![
        ("金蝶云星空如何配置期货点价", "今天天气真好"),
        ("PCR审批流程配置", "我喜欢吃火锅"),
    ];

    println!("=== Testing semantic similarity ===");

    for (a, b) in &similar_pairs {
        let sim = compute_similarity(&mut model, a, b);
        println!("  Similar pair: \"{}\" vs \"{}\" → {:.4}", a, b, sim);
        assert!(
            sim >= 0.7,
            "Related texts \"{}\" vs \"{}\" cosine similarity {:.4} < 0.7",
            a,
            b,
            sim
        );
    }

    for (a, b) in &different_pairs {
        let sim = compute_similarity(&mut model, a, b);
        println!("  Different pair: \"{}\" vs \"{}\" → {:.4}", a, b, sim);
        assert!(
            sim < 0.5,
            "Unrelated texts \"{}\" vs \"{}\" cosine similarity {:.4} >= 0.5",
            a,
            b,
            sim
        );
    }

    println!("All semantic similarity assertions passed!");
}

#[test]
fn test_vector_dimension() {
    let mut model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::BGESmallZHV15).with_show_download_progress(true),
    )
    .expect("Failed to initialize model");

    let embeddings = model
        .embed(vec!["金蝶云星空ERP系统"], None)
        .expect("Embedding failed");

    assert_eq!(
        embeddings[0].len(),
        512,
        "Expected 512-dim vector, got {}",
        embeddings[0].len()
    );

    // Verify vector is not all zeros
    let has_nonzero = embeddings[0].iter().any(|&x| x.abs() > 1e-6);
    assert!(has_nonzero, "Vector should not be all zeros");
}
