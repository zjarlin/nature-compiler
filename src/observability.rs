use serde::{Deserialize, Serialize};

/// 编译器内部可观测阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileStage {
    SourceContract,
    Inference,
    CapabilityResolution,
    BlueprintPolicy,
    RustGeneration,
}

/// 单个编译阶段的结束状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileStageStatus {
    Succeeded,
    Failed,
}

/// 编译阶段的确定性耗时记录。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileStageObservation {
    pub stage: CompileStage,
    pub status: CompileStageStatus,
    pub duration_ms: u64,
}

/// 推导器的执行模式。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceMode {
    Deterministic,
    Remote,
}

/// 推导器和远程模型返回的用量指标。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceMetrics {
    pub engine: String,
    pub mode: InferenceMode,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub reused_semantics: u64,
}

impl InferenceMetrics {
    pub fn deterministic(engine: impl Into<String>) -> Self {
        Self {
            engine: engine.into(),
            mode: InferenceMode::Deterministic,
            model: None,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cached_input_tokens: 0,
            reused_semantics: 0,
        }
    }
}

/// 一次纯编译调用的完整可观测结果。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileTrace {
    pub stages: Vec<CompileStageObservation>,
    pub inference: InferenceMetrics,
}

impl Default for CompileTrace {
    fn default() -> Self {
        Self {
            stages: Vec::new(),
            inference: InferenceMetrics::deterministic("unresolved"),
        }
    }
}
