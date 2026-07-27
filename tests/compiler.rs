use std::{fs, process::Command, sync::Arc};

use anyhow::{Context, Result, bail};
use nature_compiler::{
    AppliedDefault, Blueprint, CapabilityCatalog, CapabilityProvider, CompileRequest, CompileStage,
    Compiler, CompilerCatalog, Diagnostic, FixtureMapProvider, InferenceMode,
    MotherTongueInferenceEngine, SemanticDescriptor, ViewLayout,
};
use serde_json::{Value, json};
use tempfile::TempDir;

const ENVIRONMENT_SOURCE: &str = include_str!("fixtures/environment.txt");
const ENVIRONMENT_ARTIFACT_HASH: &str = include_str!("fixtures/environment.sha256");

fn compiler() -> Compiler {
    Compiler::new(
        Arc::new(MotherTongueInferenceEngine),
        CompilerCatalog::with_fixture_map(),
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

    assert!(result.artifacts.is_some(), "诊断: {:?}", result.diagnostics);
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
    assert_eq!(
        result
            .trace
            .stages
            .iter()
            .map(|observation| observation.stage)
            .collect::<Vec<_>>(),
        [
            CompileStage::SourceContract,
            CompileStage::Inference,
            CompileStage::CapabilityResolution,
            CompileStage::BlueprintPolicy,
            CompileStage::RustGeneration,
        ]
    );
    assert_eq!(result.trace.inference.mode, InferenceMode::Deterministic);
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
    assert_eq!(previous.inference_decisions, current.inference_decisions);
    assert!(second.trace.inference.reused_semantics > 0);
    assert_eq!(first.artifacts, second.artifacts);
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
    let compiler = Compiler::new(
        Arc::new(MotherTongueInferenceEngine),
        CompilerCatalog::new(catalog, Vec::new(), Vec::new()),
    );
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
async fn compiles_full_stack_user_domain_without_machine_protocol() -> Result<()> {
    let source = r#"领域：用户管理

需求：
1. 用户可以注册和登录
2. 管理员可以查询、修改和停用用户

建模：用户
1. 用户名：文本，必填，唯一
2. 密码：密码，必填
3. 邮箱：文本，邮箱格式
4. 权限等级：字典，显示母语标签

操作：
1. 注册用户时校验用户名和邮箱，然后保存用户
2. 登录时校验密码并返回登录结果

界面：用户列表
1. 使用表格展示用户名、邮箱和权限等级

界面：用户资料
1. 使用表单管理用户信息

导航：
1. 在“组织管理”下面显示“用户管理”
2. 用户列表作为默认页面

权限：
1. 用户只能管理自己的资料
2. 管理员可以管理全部用户"#;
    let result = compiler()
        .compile(CompileRequest {
            source_text: source.to_string(),
            previous_blueprint: None,
        })
        .await?;
    let blueprint = require_blueprint(result.blueprint.as_ref())?;

    assert!(result.artifacts.is_some(), "诊断: {:?}", result.diagnostics);
    assert_eq!(
        blueprint.application.domain.descriptor.native_name,
        "用户管理"
    );
    assert_eq!(blueprint.application.views.len(), 2);
    assert_eq!(blueprint.application.views[0].layout, ViewLayout::Table);
    assert_eq!(blueprint.application.navigation.section_label, "组织管理");
    assert!(
        blueprint
            .application
            .interfaces
            .iter()
            .all(|interface| interface.path.starts_with("/api/app/"))
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
