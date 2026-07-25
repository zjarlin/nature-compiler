use std::{sync::Arc, time::Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    ArtifactSet, Blueprint, BreakingChange, CapabilityCatalog, CompileStage,
    CompileStageObservation, CompileStageStatus, CompileTrace, Diagnostic, DiagnosticLevel, Encode,
    ImpactArea, InferenceEngine, RustBackend, policy,
};

/// 编译请求只接收母语原文和可选上一版中间表示。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileRequest {
    pub source_text: String,
    pub previous_blueprint: Option<Blueprint>,
}

/// 完整编译结果；存在错误诊断时不会返回生成物。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileResult {
    pub blueprint: Option<Blueprint>,
    pub diagnostics: Vec<Diagnostic>,
    pub breaking_changes: Vec<BreakingChange>,
    pub artifacts: Option<ArtifactSet>,
    pub trace: CompileTrace,
}

/// 编排推导、能力解析、策略校验和确定性 Rust 后端。
pub struct Compiler {
    inference: Arc<dyn InferenceEngine>,
    capabilities: CapabilityCatalog,
    backend: RustBackend,
}

impl Compiler {
    pub fn new(inference: Arc<dyn InferenceEngine>, capabilities: CapabilityCatalog) -> Self {
        Self {
            inference,
            capabilities,
            backend: RustBackend,
        }
    }

    /// 执行无文件系统副作用的完整编译。
    pub async fn compile(&self, request: CompileRequest) -> Result<CompileResult> {
        let mut trace = CompileTrace::default();
        let stage_started = Instant::now();
        let mut diagnostics = policy::validate_source_contract(&request.source_text);
        trace_stage(
            &mut trace,
            CompileStage::SourceContract,
            stage_started,
            !has_errors(&diagnostics),
        );
        if has_errors(&diagnostics) {
            return Ok(CompileResult {
                blueprint: None,
                diagnostics,
                breaking_changes: Vec::new(),
                artifacts: None,
                trace,
            });
        }

        let stage_started = Instant::now();
        let inference = self
            .inference
            .infer(&request.source_text, request.previous_blueprint.as_ref())
            .await?;
        trace_stage(&mut trace, CompileStage::Inference, stage_started, true);
        trace.inference = inference.metrics;
        let mut blueprint = inference.blueprint;
        diagnostics.extend(inference.diagnostics);

        let stage_started = Instant::now();
        for requirement in blueprint.capabilities.clone() {
            match self.capabilities.resolve(
                &requirement.descriptor.native_name,
                &requirement.source_phrase,
            ) {
                Ok(provider) => {
                    diagnostics.extend(provider.validate(&blueprint));
                    provider.lower(&mut blueprint)?;
                }
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
        trace_stage(
            &mut trace,
            CompileStage::CapabilityResolution,
            stage_started,
            !has_errors(&diagnostics),
        );

        let stage_started = Instant::now();
        diagnostics.extend(policy::validate_blueprint(&blueprint));
        let breaking_changes = request
            .previous_blueprint
            .as_ref()
            .map(|previous| detect_breaking_changes(previous, &blueprint))
            .unwrap_or_default();
        trace_stage(
            &mut trace,
            CompileStage::BlueprintPolicy,
            stage_started,
            !has_errors(&diagnostics),
        );
        if has_errors(&diagnostics) {
            return Ok(CompileResult {
                blueprint: Some(blueprint),
                diagnostics,
                breaking_changes,
                artifacts: None,
                trace,
            });
        }

        let stage_started = Instant::now();
        let artifacts = self.backend.generate(&blueprint)?;
        trace_stage(
            &mut trace,
            CompileStage::RustGeneration,
            stage_started,
            true,
        );
        Ok(CompileResult {
            blueprint: Some(blueprint),
            diagnostics,
            breaking_changes,
            artifacts: Some(artifacts),
            trace,
        })
    }
}

fn trace_stage(trace: &mut CompileTrace, stage: CompileStage, started: Instant, succeeded: bool) {
    let elapsed = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    trace.stages.push(CompileStageObservation {
        stage,
        status: if succeeded {
            CompileStageStatus::Succeeded
        } else {
            CompileStageStatus::Failed
        },
        duration_ms: elapsed,
    });
}

fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.level == DiagnosticLevel::Error)
}

fn detect_breaking_changes(previous: &Blueprint, current: &Blueprint) -> Vec<BreakingChange> {
    let mut changes = Vec::new();
    for (old, new) in previous.structs.iter().zip(&current.structs) {
        compare_descriptor(
            &old.descriptor.native_name,
            old.encode(),
            &new.descriptor.native_name,
            new.encode(),
            &mut changes,
        );
        for (old_field, new_field) in old.fields.iter().zip(&new.fields) {
            compare_descriptor(
                &old_field.descriptor.native_name,
                old_field.descriptor.encode(),
                &new_field.descriptor.native_name,
                new_field.descriptor.encode(),
                &mut changes,
            );
        }
    }
    changes
}

fn compare_descriptor(
    old_name: &str,
    old_code: &str,
    new_name: &str,
    new_code: &str,
    changes: &mut Vec<BreakingChange>,
) {
    if old_code == new_code {
        return;
    }
    changes.push(BreakingChange {
        subject: format!("{old_name} → {new_name}"),
        previous_code: old_code.to_string(),
        current_code: new_code.to_string(),
        impacts: vec![
            ImpactArea::Database,
            ImpactArea::Api,
            ImpactArea::Dictionary,
            ImpactArea::DeviceBinding,
        ],
        message: "母语名称变化导致代码身份重算，所有内部引用必须同步迁移。".to_string(),
    });
}
