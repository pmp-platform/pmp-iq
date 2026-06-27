//! Domain types for user-configured analysis vocabulary: allowed entity kinds
//! and the properties to extract per entity type.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// One allowed `kind`/`app_type`/`ecosystem` value for an entity type, with a
/// stable id, a friendly name, a description, and a free-form `config` object
/// (used by diagram / observability-signal kinds).
#[derive(Debug, Clone)]
pub struct EntityKind {
    pub id: Uuid,
    pub entity_type: String,
    pub kind_id: String,
    pub name: String,
    pub description: String,
    pub config: Value,
}

/// New/updated-kind input.
#[derive(Debug, Clone)]
pub struct EntityKindInput {
    pub entity_type: String,
    pub kind_id: String,
    pub name: String,
    pub description: String,
    pub config: Value,
}

/// One property the analyzer should extract into an entity's `metadata`.
#[derive(Debug, Clone)]
pub struct EntityProperty {
    pub id: Uuid,
    pub entity_type: String,
    pub prop_id: String,
    pub name: String,
    pub description: String,
    pub data_type: DataType,
}

/// New/updated-property input.
#[derive(Debug, Clone)]
pub struct EntityPropertyInput {
    pub entity_type: String,
    pub prop_id: String,
    pub name: String,
    pub description: String,
    pub data_type: DataType,
}

/// The supported property data types (guides the LLM's structured output).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    String,
    Number,
    Boolean,
    Date,
    ArrayOfStrings,
}

impl DataType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DataType::String => "string",
            DataType::Number => "number",
            DataType::Boolean => "boolean",
            DataType::Date => "date",
            DataType::ArrayOfStrings => "array_of_strings",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "string" => Ok(DataType::String),
            "number" => Ok(DataType::Number),
            "boolean" => Ok(DataType::Boolean),
            "date" => Ok(DataType::Date),
            "array_of_strings" => Ok(DataType::ArrayOfStrings),
            other => Err(format!("unknown data type '{other}'")),
        }
    }

    /// Human-readable label for the prompt (e.g. "array of strings").
    pub fn label(&self) -> &'static str {
        match self {
            DataType::ArrayOfStrings => "array of strings",
            other => other.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_type_round_trips() {
        for dt in [
            DataType::String,
            DataType::Number,
            DataType::Boolean,
            DataType::Date,
            DataType::ArrayOfStrings,
        ] {
            assert_eq!(DataType::parse(dt.as_str()).unwrap(), dt);
        }
        assert!(DataType::parse("bogus").is_err());
    }
}
