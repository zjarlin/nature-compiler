use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CapabilityCatalog, SemanticDescriptor};

/// 提供给推导器和策略校验器的宿主能力描述。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticCapabilityDefinition {
    pub descriptor: SemanticDescriptor,
    pub aliases: Vec<String>,
    pub config_schema: Value,
}

/// 编译器持有的完整宿主能力目录。
#[derive(Clone, Default)]
pub struct CompilerCatalog {
    pub capabilities: CapabilityCatalog,
    pub operation_capabilities: Vec<SemanticCapabilityDefinition>,
    pub view_components: Vec<SemanticCapabilityDefinition>,
}

impl CompilerCatalog {
    pub fn new(
        capabilities: CapabilityCatalog,
        operation_capabilities: Vec<SemanticCapabilityDefinition>,
        view_components: Vec<SemanticCapabilityDefinition>,
    ) -> Self {
        Self {
            capabilities,
            operation_capabilities,
            view_components,
        }
    }

    pub fn with_fixture_map() -> Self {
        Self {
            capabilities: CapabilityCatalog::with_fixture_map(),
            operation_capabilities: Vec::new(),
            view_components: Vec::new(),
        }
    }
}
