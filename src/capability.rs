use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};

use crate::{AppliedDefault, Blueprint, Diagnostic, SemanticDescriptor};

/// 可由 Rudi 收集的开放能力提供者。
pub trait CapabilityProvider: Send + Sync {
    fn descriptor(&self) -> SemanticDescriptor;

    fn aliases(&self) -> &'static [&'static str];

    fn config_schema(&self) -> Value;

    fn defaults(&self) -> Vec<AppliedDefault>;

    fn validate(&self, blueprint: &Blueprint) -> Vec<Diagnostic>;

    fn lower(&self, blueprint: &mut Blueprint) -> Result<()>;
}

/// 由宿主注入的能力目录，编译器本身不硬编码扩展分支。
#[derive(Clone, Default)]
pub struct CapabilityCatalog {
    providers: Vec<Arc<dyn CapabilityProvider>>,
}

impl CapabilityCatalog {
    pub fn new(providers: Vec<Arc<dyn CapabilityProvider>>) -> Self {
        Self { providers }
    }

    pub fn with_fixture_map() -> Self {
        Self::new(vec![Arc::new(FixtureMapProvider)])
    }

    /// 按母语名称和别名解析唯一能力，歧义时拒绝猜测。
    pub fn resolve(
        &self,
        native_name: &str,
        source_phrase: &str,
    ) -> Result<Arc<dyn CapabilityProvider>, Diagnostic> {
        let matched = self
            .providers
            .iter()
            .filter(|provider| provider_matches(provider.as_ref(), native_name, source_phrase))
            .cloned()
            .collect::<Vec<_>>();
        match matched.as_slice() {
            [provider] => Ok(Arc::clone(provider)),
            [] => Err(Diagnostic::error(
                native_name,
                "没有已注册能力可以解释这段数据获取语义。",
            )),
            _ => Err(Diagnostic::error(
                native_name,
                "多个已注册能力同时匹配，无法安全选择；请补充更明确的母语约束。",
            )),
        }
    }
}

fn provider_matches(
    provider: &dyn CapabilityProvider,
    native_name: &str,
    source_phrase: &str,
) -> bool {
    let descriptor = provider.descriptor();
    descriptor.native_name == native_name
        || provider
            .aliases()
            .iter()
            .any(|alias| native_name.contains(alias) || source_phrase.contains(alias))
}

/// 首版唯一的数据源能力，从宿主提供的 Map 中读取模拟原值。
#[derive(Clone, Copy, Debug, Default)]
pub struct FixtureMapProvider;

impl CapabilityProvider for FixtureMapProvider {
    fn descriptor(&self) -> SemanticDescriptor {
        SemanticDescriptor::new("模拟采集", "fixture_map", "fixture_map")
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["模拟采集", "模拟数据", "测试数据"]
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": {
                "type": ["number", "integer", "string", "boolean"]
            }
        })
    }

    fn defaults(&self) -> Vec<AppliedDefault> {
        vec![AppliedDefault {
            subject: "模拟采集".to_string(),
            value: "严格键匹配".to_string(),
            reason: "缺字段必须失败，不能以零值伪造有效遥测".to_string(),
        }]
    }

    fn validate(&self, blueprint: &Blueprint) -> Vec<Diagnostic> {
        if blueprint.bindings.is_empty() {
            vec![Diagnostic::error(
                "模拟采集",
                "模拟采集至少需要一个字段绑定。",
            )]
        } else {
            Vec::new()
        }
    }

    fn lower(&self, blueprint: &mut Blueprint) -> Result<()> {
        blueprint.defaults.extend(self.defaults());
        Ok(())
    }
}
