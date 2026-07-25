use std::{fs, process::Command, sync::Arc};

use anyhow::{Context, Result, bail};
use nature_compiler::{
    AppliedDefault, Blueprint, CapabilityCatalog, CapabilityProvider, CompileRequest, Compiler,
    Diagnostic, FixtureMapProvider, MotherTongueInferenceEngine, SemanticDescriptor,
};
use serde_json::{Value, json};
use tempfile::TempDir;

const ENVIRONMENT_SOURCE: &str = include_str!("fixtures/environment.txt");
const ENVIRONMENT_ARTIFACT_HASH: &str = include_str!("fixtures/environment.sha256");

fn compiler() -> Compiler {
    Compiler::new(
        Arc::new(MotherTongueInferenceEngine),
        CapabilityCatalog::with_fixture_map(),
    )
}

#[tokio::test]
async fn compiles_mother_tongue_environment_model() -> Result<()> {
    let result = compiler()
        .compile(CompileRequest {
            source_text: ENVIRONMENT_SOURCE.to_string(),
            previous_blueprint: None,
        })
        .await?;

    assert!(result.artifacts.is_some());
    let blueprint = require_blueprint(result.blueprint.as_ref())?;
    assert_eq!(
        blueprint.structs[0].descriptor.code,
        "environment_telemetry"
    );
    assert_eq!(
        blueprint.structs[0].fields[0].descriptor.code,
        "temperature"
    );
    assert_eq!(blueprint.bindings[0].source.code, "temp_x10");
    assert_eq!(blueprint.bindings[1].source.code, "humidity_x10");
    let Some(artifacts) = result.artifacts.as_ref() else {
        bail!("环境采集应生成 Rust artifact");
    };
    assert_eq!(artifacts.hash, ENVIRONMENT_ARTIFACT_HASH.trim());
    Ok(())
}

#[tokio::test]
async fn stable_semantics_reuse_previous_inference() -> Result<()> {
    let first = compiler()
        .compile(CompileRequest {
            source_text: ENVIRONMENT_SOURCE.to_string(),
            previous_blueprint: None,
        })
        .await?;
    let previous = require_blueprint(first.blueprint.as_ref())?.clone();
    let second = compiler()
        .compile(CompileRequest {
            source_text: ENVIRONMENT_SOURCE.to_string(),
            previous_blueprint: Some(previous.clone()),
        })
        .await?;
    let current = require_blueprint(second.blueprint.as_ref())?;

    assert_eq!(previous.structs, current.structs);
    assert!(current.inference_decisions.iter().any(|item| item.reused));
    Ok(())
}

#[tokio::test]
async fn rename_reencodes_and_reports_external_impacts() -> Result<()> {
    let first = compiler()
        .compile(CompileRequest {
            source_text: ENVIRONMENT_SOURCE.to_string(),
            previous_blueprint: None,
        })
        .await?;
    let previous = require_blueprint(first.blueprint.as_ref())?.clone();
    let renamed_source = ENVIRONMENT_SOURCE.replace("温度", "室温");
    let second = compiler()
        .compile(CompileRequest {
            source_text: renamed_source,
            previous_blueprint: Some(previous),
        })
        .await?;
    let current = require_blueprint(second.blueprint.as_ref())?;

    assert_ne!(current.structs[0].fields[0].descriptor.code, "temperature");
    assert!(
        second
            .breaking_changes
            .iter()
            .any(|change| change.previous_code == "temperature")
    );
    Ok(())
}

#[tokio::test]
async fn rejects_machine_protocol_in_product_source() -> Result<()> {
    let result = compiler()
        .compile(CompileRequest {
            source_text: "需求：用户\nkind: iot.modbus.register".to_string(),
            previous_blueprint: None,
        })
        .await?;

    assert!(result.artifacts.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.message.contains("不能填写"))
    );
    Ok(())
}

#[tokio::test]
async fn capability_ambiguity_fails_before_generation() -> Result<()> {
    let catalog = CapabilityCatalog::new(vec![
        Arc::new(FixtureMapProvider),
        Arc::new(SecondFixtureProvider),
    ]);
    let compiler = Compiler::new(Arc::new(MotherTongueInferenceEngine), catalog);
    let result = compiler
        .compile(CompileRequest {
            source_text: ENVIRONMENT_SOURCE.to_string(),
            previous_blueprint: None,
        })
        .await?;

    assert!(result.artifacts.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.message.contains("多个"))
    );
    Ok(())
}

#[tokio::test]
async fn generated_artifacts_compile_and_run_their_tests() -> Result<()> {
    let result = compiler()
        .compile(CompileRequest {
            source_text: ENVIRONMENT_SOURCE.to_string(),
            previous_blueprint: None,
        })
        .await?;
    let Some(artifacts) = result.artifacts else {
        bail!("环境采集应生成 Rust artifact");
    };
    let temp = TempDir::new().context("创建临时生成 crate 失败")?;
    for file in artifacts.files {
        let destination = temp.path().join(file.relative_path);
        let Some(parent) = destination.parent() else {
            bail!("生成文件没有父目录: {}", destination.display());
        };
        fs::create_dir_all(parent)
            .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        fs::write(&destination, file.source)
            .with_context(|| format!("写入生成文件失败: {}", destination.display()))?;
    }
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "az-aio-nature-generated"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .context("写入临时 Cargo.toml 失败")?;
    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .current_dir(temp.path())
        .output()
        .context("执行生成 crate 测试失败")?;
    if !output.status.success() {
        bail!(
            "生成 crate 未通过测试:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn require_blueprint(value: Option<&Blueprint>) -> Result<&Blueprint> {
    value.context("编译结果缺少 Blueprint")
}

#[derive(Clone, Copy, Debug)]
struct SecondFixtureProvider;

impl CapabilityProvider for SecondFixtureProvider {
    fn descriptor(&self) -> SemanticDescriptor {
        SemanticDescriptor::new("另一个模拟采集", "second_fixture", "second_fixture")
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["模拟采集"]
    }

    fn config_schema(&self) -> Value {
        json!({"type": "object"})
    }

    fn defaults(&self) -> Vec<AppliedDefault> {
        Vec::new()
    }

    fn validate(&self, _blueprint: &Blueprint) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn lower(&self, _blueprint: &mut Blueprint) -> Result<()> {
        Ok(())
    }
}
