use std::collections::BTreeMap;

use convert_case::{Case, Casing};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Encode;

/// 母语名称及编译器推导出的英文身份。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticDescriptor {
    pub native_name: String,
    pub english_stem: String,
    pub code: String,
}

impl SemanticDescriptor {
    /// 构造已经完成规范化的语义描述符。
    pub fn new(
        native_name: impl Into<String>,
        english_stem: impl Into<String>,
        code: impl Into<String>,
    ) -> Self {
        Self {
            native_name: native_name.into(),
            english_stem: english_stem.into(),
            code: code.into(),
        }
    }
}

impl Encode for SemanticDescriptor {
    fn encode(&self) -> &str {
        &self.code
    }
}

/// 把 Agent 推导的英文 stem 规范化，并稳定解决同一批次内的冲突。
#[derive(Clone, Debug, Default)]
pub struct DescriptorEncoder {
    claimed_codes: BTreeMap<String, String>,
}

impl DescriptorEncoder {
    pub fn describe(&mut self, native_name: &str, english_stem: &str) -> SemanticDescriptor {
        let base_code = normalize_inferred_stem(english_stem, native_name);
        let code = match self.claimed_codes.get(&base_code) {
            Some(claimed_by) if claimed_by != native_name => {
                format!("{base_code}_{}", short_hash(native_name))
            }
            _ => base_code,
        };
        self.claimed_codes
            .insert(code.clone(), native_name.to_string());
        SemanticDescriptor::new(native_name, english_stem, code)
    }

    pub fn reserve(&mut self, descriptor: &SemanticDescriptor) {
        self.claimed_codes
            .insert(descriptor.code.clone(), descriptor.native_name.clone());
    }
}

/// 应用 Rust `snake_case` 与合法标识符约束，不接受 Agent 直接指定 code。
pub fn normalize_inferred_stem(english_stem: &str, native_name: &str) -> String {
    let already_snake = english_stem.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
    });
    let normalized = if already_snake {
        english_stem.to_string()
    } else {
        english_stem.to_case(Case::Snake)
    };
    let filtered = normalized
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let candidate = filtered
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if candidate.is_empty() {
        format!("semantic_{}", short_hash(native_name))
    } else if candidate.starts_with(|character: char| character.is_ascii_digit()) {
        format!("semantic_{candidate}")
    } else {
        candidate
    }
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{:x}", digest)[..8].to_string()
}
