use std::collections::BTreeMap;

use crate::{Blueprint, Diagnostic, Encode};

/// 拒绝把机器协议重新泄漏到产品输入。
pub fn validate_source_contract(source_text: &str) -> Vec<Diagnostic> {
    let forbidden = [
        ("kind:", "类型判别字段"),
        ("kind：", "类型判别字段"),
        ("dictionary:", "字典编码"),
        ("dictionary：", "字典编码"),
        ("provider_id", "能力提供者标识"),
        ("rust_type", "Rust 类型"),
        ("table_name", "数据库表名"),
        ("code:", "显式代码"),
        ("code：", "显式代码"),
    ];
    forbidden
        .into_iter()
        .filter(|(needle, _)| source_text.contains(needle))
        .map(|(_, label)| {
            Diagnostic::error(
                "母语输入",
                format!("产品输入不能填写{label}，该信息必须由编译器推导。"),
            )
        })
        .collect()
}

/// 校验中间表示的引用完整性与代码唯一性。
pub fn validate_blueprint(blueprint: &Blueprint) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut claimed = BTreeMap::<String, String>::new();
    for definition in &blueprint.structs {
        claim_code(
            definition.encode(),
            &definition.descriptor.native_name,
            &mut claimed,
            &mut diagnostics,
        );
        for field in &definition.fields {
            let qualified = format!("{}.{}", definition.encode(), field.encode());
            claim_code(
                &qualified,
                &field.descriptor.native_name,
                &mut claimed,
                &mut diagnostics,
            );
        }
    }
    for definition in &blueprint.enums {
        claim_code(
            definition.encode(),
            &definition.descriptor.native_name,
            &mut claimed,
            &mut diagnostics,
        );
    }
    for function in &blueprint.functions {
        claim_code(
            function.encode(),
            &function.descriptor.native_name,
            &mut claimed,
            &mut diagnostics,
        );
    }

    for binding in &blueprint.bindings {
        let owner_exists = blueprint
            .structs
            .iter()
            .any(|definition| definition.descriptor == binding.owner);
        let field_exists = blueprint.structs.iter().any(|definition| {
            definition.descriptor == binding.owner
                && definition
                    .fields
                    .iter()
                    .any(|field| field.descriptor == binding.field)
        });
        if !owner_exists || !field_exists {
            diagnostics.push(Diagnostic::error(
                &binding.field.native_name,
                "字段绑定引用了不存在的结构或字段。",
            ));
        }
    }
    diagnostics
}

fn claim_code(
    code: &str,
    native_name: &str,
    claimed: &mut BTreeMap<String, String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(previous) = claimed.insert(code.to_string(), native_name.to_string())
        && previous != native_name
    {
        diagnostics.push(Diagnostic::error(
            native_name,
            format!("代码身份 {code} 与“{previous}”冲突。"),
        ));
    }
}
