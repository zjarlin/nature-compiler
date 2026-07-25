use serde::{Deserialize, Serialize};

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
