//! Orchestrates the analysis-config repositories and builds the in-memory
//! [`AnalysisConfig`] consumed by the analyzer.

use super::model::{EntityKind, EntityKindInput, EntityProperty, EntityPropertyInput};
use super::repository::{EntityKindRepository, EntityPropertyRepository};
use crate::error::AppError;
use crate::platform::{AnalysisConfig, KindDef, PropertyDef};
use std::sync::Arc;
use uuid::Uuid;

/// CRUD over the config tables + assembly of the analyzer's [`AnalysisConfig`].
#[derive(Clone)]
pub struct AnalysisConfigService {
    kinds: Arc<dyn EntityKindRepository>,
    properties: Arc<dyn EntityPropertyRepository>,
}

impl AnalysisConfigService {
    pub fn new(
        kinds: Arc<dyn EntityKindRepository>,
        properties: Arc<dyn EntityPropertyRepository>,
    ) -> Self {
        Self { kinds, properties }
    }

    pub async fn list_kinds(&self) -> Result<Vec<EntityKind>, AppError> {
        Ok(self.kinds.list().await?)
    }

    pub async fn create_kind(&self, input: EntityKindInput) -> Result<EntityKind, AppError> {
        Ok(self.kinds.create(input).await?)
    }

    pub async fn update_kind(
        &self,
        id: Uuid,
        input: EntityKindInput,
    ) -> Result<EntityKind, AppError> {
        Ok(self.kinds.update(id, input).await?)
    }

    pub async fn delete_kind(&self, id: Uuid) -> Result<(), AppError> {
        Ok(self.kinds.delete(id).await?)
    }

    pub async fn list_properties(&self) -> Result<Vec<EntityProperty>, AppError> {
        Ok(self.properties.list().await?)
    }

    pub async fn create_property(
        &self,
        input: EntityPropertyInput,
    ) -> Result<EntityProperty, AppError> {
        Ok(self.properties.create(input).await?)
    }

    pub async fn update_property(
        &self,
        id: Uuid,
        input: EntityPropertyInput,
    ) -> Result<EntityProperty, AppError> {
        Ok(self.properties.update(id, input).await?)
    }

    pub async fn delete_property(&self, id: Uuid) -> Result<(), AppError> {
        Ok(self.properties.delete(id).await?)
    }

    /// Build the analyzer config: allowed kinds + properties grouped by entity.
    pub async fn load(&self) -> Result<AnalysisConfig, AppError> {
        let mut cfg = AnalysisConfig::default();
        for kind in self.kinds.list().await? {
            cfg.kinds.entry(kind.entity_type).or_default().push(KindDef {
                kind_id: kind.kind_id,
                name: kind.name,
                description: kind.description,
            });
        }
        for prop in self.properties.list().await? {
            cfg.properties.entry(prop.entity_type).or_default().push(PropertyDef {
                prop_id: prop.prop_id,
                name: prop.name,
                description: prop.description,
                data_type: prop.data_type.label().to_string(),
            });
        }
        Ok(cfg)
    }
}
