//! Pure helpers for semantic search (M40): the per-entity summary text, its
//! hash (to skip re-embedding unchanged entities), cosine similarity, and
//! greedy similarity clustering for duplicate detection.

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// An entity to embed: a stable type/id and the compact text that represents it.
#[derive(Debug, Clone)]
pub struct EmbeddingSource {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub summary: String,
}

/// A stored embedding (raw vector kept as a little-endian f32 blob).
#[derive(Debug, Clone)]
pub struct EntityEmbedding {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub vector: Vec<f32>,
    pub summary_hash: String,
}

/// A nearest-neighbour search result.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbour {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub score: f32,
}

/// Build a compact, deterministic summary so the hash is stable across syncs
/// unless the meaningful fields change.
pub fn build_summary(name: &str, kind: &str, description: &str) -> String {
    let mut parts = vec![name.trim().to_string()];
    if !kind.trim().is_empty() {
        parts.push(format!("[{}]", kind.trim()));
    }
    if !description.trim().is_empty() {
        parts.push(description.trim().to_string());
    }
    parts.join(" ")
}

/// Stable hex digest of a summary (skip re-embedding unchanged summaries).
pub fn summary_hash(summary: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(summary.as_bytes());
    hex::encode(hasher.finalize())
}

/// Serialize a vector to a little-endian f32 blob for storage.
pub fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Deserialize a little-endian f32 blob back into a vector.
pub fn from_blob(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Cosine similarity in `[-1, 1]`; 0 when either vector is empty/zero or the
/// dimensions differ.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Rank `candidates` by cosine similarity to `query`, descending, top `k`.
pub fn rank(query: &[f32], candidates: &[EntityEmbedding], k: usize) -> Vec<Neighbour> {
    let mut scored: Vec<Neighbour> = candidates
        .iter()
        .map(|c| Neighbour {
            entity_type: c.entity_type.clone(),
            entity_id: c.entity_id,
            score: cosine(query, &c.vector),
        })
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

/// Greedy union-find clustering: items whose pairwise cosine ≥ `threshold` are
/// grouped. Only clusters with ≥ 2 members (true overlaps) are returned.
pub fn cluster(items: &[EntityEmbedding], threshold: f32) -> Vec<Vec<Uuid>> {
    let n = items.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in (i + 1)..n {
            if cosine(&items[i].vector, &items[j].vector) >= threshold {
                union(&mut parent, i, j);
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<Uuid>> = Default::default();
    for (i, item) in items.iter().enumerate() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(item.entity_id);
    }
    groups.into_values().filter(|g| g.len() >= 2).collect()
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[ra] = rb;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emb(id: Uuid, v: Vec<f32>) -> EntityEmbedding {
        EntityEmbedding { entity_type: "application".into(), entity_id: id, vector: v, summary_hash: "h".into() }
    }

    #[test]
    fn blob_round_trips() {
        let v = vec![1.0, -2.5, 3.25];
        assert_eq!(from_blob(&to_blob(&v)), v);
    }

    #[test]
    fn cosine_basics() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0); // dim mismatch
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0); // zero vector
    }

    #[test]
    fn rank_orders_by_similarity() {
        let near = Uuid::new_v4();
        let far = Uuid::new_v4();
        let candidates = vec![emb(far, vec![0.0, 1.0]), emb(near, vec![1.0, 0.1])];
        let ranked = rank(&[1.0, 0.0], &candidates, 10);
        assert_eq!(ranked[0].entity_id, near);
        assert_eq!(ranked[1].entity_id, far);
        assert_eq!(rank(&[1.0, 0.0], &candidates, 1).len(), 1);
    }

    #[test]
    fn cluster_groups_similar_and_drops_singletons() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        // a & b nearly identical; c orthogonal.
        let items = vec![emb(a, vec![1.0, 0.0]), emb(b, vec![0.99, 0.01]), emb(c, vec![0.0, 1.0])];
        let clusters = cluster(&items, 0.9);
        assert_eq!(clusters.len(), 1);
        let group = &clusters[0];
        assert!(group.contains(&a) && group.contains(&b));
        assert!(!group.contains(&c));
    }

    #[test]
    fn summary_hash_is_stable_and_changes_with_text() {
        let s = build_summary("Billing", "api", "charges cards");
        assert_eq!(summary_hash(&s), summary_hash(&s));
        assert_ne!(summary_hash(&s), summary_hash("different"));
        assert!(s.contains("Billing") && s.contains("[api]") && s.contains("charges cards"));
    }
}
