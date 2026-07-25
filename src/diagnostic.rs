use serde::{Deserialize, Serialize};

/// 编译诊断级别。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    Notice,
    Warning,
    Error,
}

/// 面向产品与评审者的母语诊断。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub subject: String,
    pub message: String,
}

impl Diagnostic {
    pub fn notice(subject: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Notice,
            subject: subject.into(),
            message: message.into(),
        }
    }

    pub fn warning(subject: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            subject: subject.into(),
            message: message.into(),
        }
    }

    pub fn error(subject: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            subject: subject.into(),
            message: message.into(),
        }
    }
}

/// 改名或重新编码可能影响的外部边界。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactArea {
    Database,
    Api,
    Dictionary,
    DeviceBinding,
}

/// 由语义差异推导出的破坏性变化。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakingChange {
    pub subject: String,
    pub previous_code: String,
    pub current_code: String,
    pub impacts: Vec<ImpactArea>,
    pub message: String,
}
