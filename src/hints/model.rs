//! Domain model for per-entity LLM hints.

use serde::Serialize;
use uuid::Uuid;

/// A free-text hint correcting/augmenting what the LLM inferred for an
/// application or one of its entities. `entity_key` is the entity's natural key
/// (its name); an empty key applies to the whole `entity_type`.
#[derive(Debug, Clone, Serialize)]
pub struct EntityHint {
    pub id: Uuid,
    pub application_id: Uuid,
    pub entity_type: String,
    pub entity_key: String,
    pub hint: String,
}

/// Fields needed to upsert a hint.
#[derive(Debug, Clone)]
pub struct EntityHintInput {
    pub application_id: Uuid,
    pub entity_type: String,
    pub entity_key: String,
    pub hint: String,
}
