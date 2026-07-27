use anyhow::{Context, Result, bail};
use convert_case::{Case, Casing};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

use crate::{
    ArtifactFile, ArtifactSet, Blueprint, FieldBinding, FieldDefinition, FieldType, ValidationRule,
    ValueTransform,
};

/// 只消费 Blueprint 的确定性 Rust 后端。
#[derive(Clone, Copy, Debug, Default)]
pub struct RustBackend;

impl RustBackend {
    pub fn generate(&self, blueprint: &Blueprint) -> Result<ArtifactSet> {
        let blueprint_json =
            serde_json::to_string_pretty(blueprint).context("序列化 Blueprint 快照失败")?;
        let application_json = serde_json::to_string_pretty(&blueprint.application)
            .context("序列化应用定义快照失败")?;
        let files = vec![
            artifact("blueprint.json", blueprint_json),
            artifact("application.json", application_json),
            artifact("src/lib.rs", render_lib()),
            artifact("src/descriptors.rs", render_descriptors(blueprint)?),
            artifact("src/structs.rs", render_structs(blueprint)?),
            artifact("src/enums.rs", render_enums(blueprint)?),
            artifact("src/validators.rs", render_validators(blueprint)?),
            artifact("src/bindings.rs", render_bindings(blueprint)?),
            artifact("src/functions.rs", render_functions(blueprint)?),
            artifact("tests/generated.rs", render_generated_tests(blueprint)?),
        ];
        Ok(ArtifactSet::new(files))
    }
}

fn artifact(relative_path: &str, source: String) -> ArtifactFile {
    ArtifactFile {
        relative_path: relative_path.to_string(),
        source,
    }
}

fn render_lib() -> String {
    r#"#![forbid(unsafe_code)]

pub mod bindings;
pub mod descriptors;
pub mod enums;
pub mod functions;
pub mod structs;
pub mod validators;
"#
    .to_string()
}

fn render_descriptors(blueprint: &Blueprint) -> Result<String> {
    let mut field_descriptors = Vec::new();
    for definition in &blueprint.structs {
        for field in &definition.fields {
            let descriptor_ident = type_ident(&format!(
                "{}_{}_field",
                definition.descriptor.code, field.descriptor.code
            ))?;
            let code = &field.descriptor.code;
            let label = &field.descriptor.native_name;
            let unit = field
                .unit
                .as_deref()
                .map(|unit| quote! { Some(#unit) })
                .unwrap_or_else(|| quote! { None });
            field_descriptors.push(quote! {
                pub struct #descriptor_ident;

                impl #descriptor_ident {
                    pub const DESCRIPTOR: Descriptor = Descriptor {
                        code: #code,
                        label: #label,
                        unit: #unit,
                    };

                    pub const fn encode() -> &'static str {
                        #code
                    }
                }

                impl Encode for #descriptor_ident {
                    fn encode(&self) -> &'static str {
                        Self::encode()
                    }
                }
            });
        }
    }
    format_tokens(quote! {
        /// 生成类型和值的稳定代码身份。
        pub trait Encode {
            fn encode(&self) -> &'static str;
        }

        /// 字段、结构和函数共享的只读语义描述。
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct Descriptor {
            pub code: &'static str,
            pub label: &'static str,
            pub unit: Option<&'static str>,
        }

        #(#field_descriptors)*
    })
}

fn render_structs(blueprint: &Blueprint) -> Result<String> {
    let mut definitions = Vec::new();
    for definition in &blueprint.structs {
        let struct_ident = type_ident(&definition.descriptor.code)?;
        let type_code = &definition.descriptor.code;
        let field_tokens = definition
            .fields
            .iter()
            .map(|field| {
                let field_ident = value_ident(&field.descriptor.code)?;
                let field_type = rust_type(&field.field_type)?;
                Ok(quote! { pub #field_ident: #field_type })
            })
            .collect::<Result<Vec<TokenStream>>>()?;
        definitions.push(quote! {
            #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct #struct_ident {
                #(#field_tokens,)*
            }

            impl #struct_ident {
                pub const fn encode() -> &'static str {
                    #type_code
                }
            }

            impl crate::descriptors::Encode for #struct_ident {
                fn encode(&self) -> &'static str {
                    Self::encode()
                }
            }
        });
    }
    format_tokens(quote! { #(#definitions)* })
}

fn render_enums(blueprint: &Blueprint) -> Result<String> {
    let mut definitions = Vec::new();
    for definition in &blueprint.enums {
        let enum_ident = type_ident(&definition.descriptor.code)?;
        let variants = definition
            .values
            .iter()
            .map(|value| type_ident(&value.descriptor.code))
            .collect::<Result<Vec<_>>>()?;
        let encode_arms = definition
            .values
            .iter()
            .zip(&variants)
            .map(|(value, variant)| {
                let code = &value.descriptor.code;
                quote! { Self::#variant => #code }
            })
            .collect::<Vec<_>>();
        definitions.push(quote! {
            #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
            pub enum #enum_ident {
                #(#variants,)*
            }

            impl crate::descriptors::Encode for #enum_ident {
                fn encode(&self) -> &'static str {
                    match self {
                        #(#encode_arms,)*
                    }
                }
            }
        });
    }
    format_tokens(quote! { #(#definitions)* })
}

fn render_validators(blueprint: &Blueprint) -> Result<String> {
    let mut validators = Vec::new();
    for definition in &blueprint.structs {
        let type_ident = type_ident(&definition.descriptor.code)?;
        let function_ident = value_ident(&format!("validate_{}", definition.descriptor.code))?;
        let mut rules = Vec::new();
        for field in &definition.fields {
            let field_ident = value_ident(&field.descriptor.code)?;
            let field_label = &field.descriptor.native_name;
            if field.required
                && matches!(
                    field.field_type,
                    FieldType::String
                        | FieldType::Password
                        | FieldType::Email
                        | FieldType::Dictionary
                )
            {
                rules.push(quote! {
                    if value.#field_ident.trim().is_empty() {
                        anyhow::bail!(concat!(#field_label, "不能为空"));
                    }
                });
            }
            for validation in &field.validations {
                if let ValidationRule::NumberRange { minimum, maximum } = validation {
                    rules.push(quote! {
                        if value.#field_ident < #minimum || value.#field_ident > #maximum {
                            anyhow::bail!(
                                concat!(#field_label, "超出允许范围 {} 到 {}"),
                                #minimum,
                                #maximum,
                            );
                        }
                    });
                }
                if matches!(validation, ValidationRule::Email) {
                    rules.push(quote! {
                        if !value.#field_ident.contains('@') {
                            anyhow::bail!(concat!(#field_label, "不是有效邮箱"));
                        }
                    });
                }
            }
        }
        validators.push(quote! {
            pub fn #function_ident(value: &crate::structs::#type_ident) -> anyhow::Result<()> {
                #(#rules)*
                Ok(())
            }
        });
    }
    format_tokens(quote! { #(#validators)* })
}

fn render_bindings(blueprint: &Blueprint) -> Result<String> {
    let mut decoders = Vec::new();
    for definition in &blueprint.structs {
        let definition_bindings = blueprint
            .bindings
            .iter()
            .filter(|binding| binding.owner == definition.descriptor)
            .collect::<Vec<_>>();
        if definition_bindings.is_empty() {
            continue;
        }
        let type_ident = type_ident(&definition.descriptor.code)?;
        let decoder_ident = value_ident(&format!("decode_{}", definition.descriptor.code))?;
        let validator_ident = value_ident(&format!("validate_{}", definition.descriptor.code))?;
        let mut assignments = Vec::new();
        for field in &definition.fields {
            let Some(binding) = blueprint.bindings.iter().find(|binding| {
                binding.owner == definition.descriptor && binding.field == field.descriptor
            }) else {
                bail!("字段 {} 缺少生成绑定", field.descriptor.native_name);
            };
            assignments.push(render_assignment(field, binding)?);
        }
        decoders.push(quote! {
            pub fn #decoder_ident(
                source: &std::collections::BTreeMap<String, serde_json::Value>,
            ) -> anyhow::Result<crate::structs::#type_ident> {
                let value = crate::structs::#type_ident {
                    #(#assignments,)*
                };
                crate::validators::#validator_ident(&value)?;
                Ok(value)
            }
        });
    }

    let helpers = quote! {
        #[allow(dead_code)]
        fn read_decimal(
            source: &std::collections::BTreeMap<String, serde_json::Value>,
            key: &str,
        ) -> anyhow::Result<f64> {
            let value = source
                .get(key)
                .ok_or_else(|| anyhow::anyhow!("缺少原始字段 {key}"))?;
            if let Some(number) = value.as_f64() {
                return Ok(number);
            }
            if let Some(text) = value.as_str() {
                return text
                    .parse::<f64>()
                    .map_err(|error| anyhow::anyhow!("原始字段 {key} 不是小数: {error}"));
            }
            anyhow::bail!("原始字段 {key} 不是小数")
        }

        #[allow(dead_code)]
        fn read_integer(
            source: &std::collections::BTreeMap<String, serde_json::Value>,
            key: &str,
        ) -> anyhow::Result<i64> {
            source
                .get(key)
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| anyhow::anyhow!("原始字段 {key} 不是整数"))
        }

        #[allow(dead_code)]
        fn read_boolean(
            source: &std::collections::BTreeMap<String, serde_json::Value>,
            key: &str,
        ) -> anyhow::Result<bool> {
            source
                .get(key)
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| anyhow::anyhow!("原始字段 {key} 不是布尔值"))
        }

        #[allow(dead_code)]
        fn read_string(
            source: &std::collections::BTreeMap<String, serde_json::Value>,
            key: &str,
        ) -> anyhow::Result<String> {
            source
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("原始字段 {key} 不是文本"))
        }
    };
    format_tokens(quote! { #helpers #(#decoders)* })
}

fn render_assignment(field: &FieldDefinition, binding: &FieldBinding) -> Result<TokenStream> {
    let field_ident = value_ident(&field.descriptor.code)?;
    let source_code = &binding.source.code;
    let read = match field.field_type {
        FieldType::Decimal => quote! { read_decimal(source, #source_code)? },
        FieldType::Integer | FieldType::Timestamp => {
            quote! { read_integer(source, #source_code)? }
        }
        FieldType::Boolean => quote! { read_boolean(source, #source_code)? },
        FieldType::String | FieldType::Password | FieldType::Email | FieldType::Dictionary => {
            quote! { read_string(source, #source_code)? }
        }
        FieldType::Json => quote! {
            source
                .get(#source_code)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("缺少原始字段 {}", #source_code))?
        },
    };
    let transformed = match binding.transform {
        ValueTransform::Identity => read,
        ValueTransform::Divide { divisor } => {
            if !matches!(field.field_type, FieldType::Decimal) {
                bail!(
                    "除法变换只能生成到小数字段: {}",
                    field.descriptor.native_name
                );
            }
            quote! { (#read) / #divisor }
        }
    };
    Ok(quote! { #field_ident: #transformed })
}

fn render_functions(blueprint: &Blueprint) -> Result<String> {
    let mut functions = Vec::new();
    for function in &blueprint.functions {
        let function_ident = value_ident(&function.descriptor.code)?;
        let descriptor_ident = type_ident(&format!("{}_function", function.descriptor.code))?;
        let output_ident = type_ident(&function.output_model.code)?;
        let decoder_ident = value_ident(&format!("decode_{}", function.output_model.code))?;
        let function_code = &function.descriptor.code;
        functions.push(quote! {
            pub struct #descriptor_ident;

            impl #descriptor_ident {
                pub const fn encode() -> &'static str {
                    #function_code
                }
            }

            impl crate::descriptors::Encode for #descriptor_ident {
                fn encode(&self) -> &'static str {
                    Self::encode()
                }
            }

            pub fn #function_ident(
                source: &std::collections::BTreeMap<String, serde_json::Value>,
            ) -> anyhow::Result<crate::structs::#output_ident> {
                crate::bindings::#decoder_ident(source)
            }
        });
    }
    format_tokens(quote! { #(#functions)* })
}

fn render_generated_tests(blueprint: &Blueprint) -> Result<String> {
    let Some(definition) = blueprint.structs.first() else {
        bail!("至少需要一个结构定义才能生成测试");
    };
    let decoder_ident = value_ident(&format!("decode_{}", definition.descriptor.code))?;
    let crate_name = quote! { az_aio_nature_generated };
    if blueprint.bindings.is_empty() {
        return render_model_tests(definition, &crate_name);
    }
    let mut valid_inserts = Vec::new();
    let mut assertions = Vec::new();
    for field in &definition.fields {
        let Some(binding) = blueprint
            .bindings
            .iter()
            .find(|binding| binding.field == field.descriptor)
        else {
            continue;
        };
        let source_code = &binding.source.code;
        let field_ident = value_ident(&field.descriptor.code)?;
        let raw_value = fixture_test_raw(field, binding);
        valid_inserts.push(quote! {
            source.insert(#source_code.to_string(), serde_json::json!(#raw_value));
        });
        assertions.push(fixture_test_assertion(field, &field_ident));
    }
    let first_key = blueprint
        .bindings
        .first()
        .map(|binding| binding.source.code.as_str())
        .unwrap_or("missing");
    let descriptor_assertion = definition
        .fields
        .first()
        .map(|field| {
            let descriptor_name = format!(
                "{}_{}_field",
                definition.descriptor.code, field.descriptor.code
            );
            let descriptor_ident = type_ident(&descriptor_name);
            let field_code = &field.descriptor.code;
            descriptor_ident.map(|descriptor_ident| {
                quote! {
                    assert_eq!(
                        #crate_name::descriptors::#descriptor_ident::encode(),
                        #field_code,
                    );
                }
            })
        })
        .transpose()?
        .unwrap_or_default();
    let range_test = render_range_rejection_test(blueprint, &crate_name, &decoder_ident)?;
    format_tokens(quote! {
        use std::collections::BTreeMap;

        #[test]
        fn fixture_map_decodes_and_validates_the_domain_value() -> anyhow::Result<()> {
            let mut source = BTreeMap::new();
            #(#valid_inserts)*
            let decoded = #crate_name::bindings::#decoder_ident(&source)?;
            #(#assertions)*
            #descriptor_assertion
            Ok(())
        }

        #[test]
        fn missing_fixture_field_is_rejected() {
            let source = BTreeMap::new();
            let result = #crate_name::bindings::#decoder_ident(&source);
            assert!(result.is_err());
            let message = result.err().map(|error| error.to_string()).unwrap_or_default();
            assert!(message.contains(#first_key));
        }

        #[test]
        fn fixture_type_error_is_rejected() {
            let mut source = BTreeMap::new();
            #(#valid_inserts)*
            source.insert(#first_key.to_string(), serde_json::json!("不是有效数值"));
            let result = #crate_name::bindings::#decoder_ident(&source);
            assert!(result.is_err());
        }

        #range_test
    })
}

fn render_model_tests(
    definition: &crate::StructDefinition,
    crate_name: &TokenStream,
) -> Result<String> {
    let type_ident = type_ident(&definition.descriptor.code)?;
    let model_code = &definition.descriptor.code;
    let validator_ident = value_ident(&format!("validate_{}", definition.descriptor.code))?;
    let assignments = definition
        .fields
        .iter()
        .map(|field| {
            let field_ident = value_ident(&field.descriptor.code)?;
            let value = model_test_value(field);
            Ok(quote! { #field_ident: #value })
        })
        .collect::<Result<Vec<_>>>()?;
    format_tokens(quote! {
        #[test]
        fn model_value_passes_generated_validation() -> anyhow::Result<()> {
            let value = #crate_name::structs::#type_ident {
                #(#assignments,)*
            };
            #crate_name::validators::#validator_ident(&value)?;
            assert_eq!(#crate_name::structs::#type_ident::encode(), #model_code);
            Ok(())
        }
    })
}

fn model_test_value(field: &FieldDefinition) -> TokenStream {
    match field.field_type {
        FieldType::Decimal => quote! { 1.0_f64 },
        FieldType::Integer | FieldType::Timestamp => quote! { 1_i64 },
        FieldType::Boolean => quote! { true },
        FieldType::String | FieldType::Password | FieldType::Dictionary => {
            quote! { "测试值".to_string() }
        }
        FieldType::Email => quote! { "test@example.com".to_string() },
        FieldType::Json => quote! { serde_json::json!({"value": 1}) },
    }
}

fn fixture_test_raw(field: &FieldDefinition, binding: &FieldBinding) -> TokenStream {
    match field.field_type {
        FieldType::Decimal => {
            let base = fixture_decimal_value(field);
            let raw = match binding.transform {
                ValueTransform::Identity => base,
                ValueTransform::Divide { divisor } => base * divisor,
            };
            quote! { #raw }
        }
        FieldType::Integer | FieldType::Timestamp => quote! { 1_i64 },
        FieldType::Boolean => quote! { true },
        FieldType::String | FieldType::Password | FieldType::Email | FieldType::Dictionary => {
            quote! { "test@example.com" }
        }
        FieldType::Json => quote! { serde_json::json!({"value": 1}) },
    }
}

fn fixture_decimal_value(field: &FieldDefinition) -> f64 {
    match field.descriptor.code.as_str() {
        "temperature" => 25.3,
        "humidity" => 61.2,
        _ => 1.0,
    }
}

fn fixture_test_assertion(field: &FieldDefinition, field_ident: &Ident) -> TokenStream {
    match field.field_type {
        FieldType::Decimal => {
            let expected = fixture_decimal_value(field);
            quote! { assert!((decoded.#field_ident - #expected).abs() < f64::EPSILON); }
        }
        FieldType::Integer | FieldType::Timestamp => {
            quote! { assert_eq!(decoded.#field_ident, 1_i64); }
        }
        FieldType::Boolean => quote! { assert!(decoded.#field_ident); },
        FieldType::String | FieldType::Password | FieldType::Email | FieldType::Dictionary => {
            quote! { assert_eq!(decoded.#field_ident, "test@example.com"); }
        }
        FieldType::Json => quote! { assert_eq!(decoded.#field_ident["value"], 1); },
    }
}

fn render_range_rejection_test(
    blueprint: &Blueprint,
    crate_name: &TokenStream,
    decoder_ident: &Ident,
) -> Result<TokenStream> {
    let ranged = blueprint
        .structs
        .iter()
        .flat_map(|definition| &definition.fields)
        .find_map(|field| {
            field.validations.iter().find_map(|validation| {
                if let ValidationRule::NumberRange { maximum, .. } = validation {
                    Some((field, *maximum))
                } else {
                    None
                }
            })
        });
    let Some((field, maximum)) = ranged else {
        return Ok(TokenStream::new());
    };
    let Some(binding) = blueprint
        .bindings
        .iter()
        .find(|binding| binding.field == field.descriptor)
    else {
        bail!("范围校验字段缺少生成绑定: {}", field.descriptor.native_name);
    };
    let source_code = &binding.source.code;
    let domain_value = maximum + 1.0;
    let raw_value = match binding.transform {
        ValueTransform::Identity => domain_value,
        ValueTransform::Divide { divisor } => domain_value * divisor,
    };
    let mut inserts = Vec::new();
    for current_field in blueprint
        .structs
        .iter()
        .flat_map(|definition| &definition.fields)
    {
        let Some(current_binding) = blueprint
            .bindings
            .iter()
            .find(|binding| binding.field == current_field.descriptor)
        else {
            continue;
        };
        let current_source_code = &current_binding.source.code;
        let current_raw = fixture_test_raw(current_field, current_binding);
        inserts.push(quote! {
            source.insert(#current_source_code.to_string(), serde_json::json!(#current_raw));
        });
    }
    Ok(quote! {
        #[test]
        fn out_of_range_fixture_value_is_rejected() {
            let mut source = BTreeMap::new();
            #(#inserts)*
            source.insert(#source_code.to_string(), serde_json::json!(#raw_value));
            let result = #crate_name::bindings::#decoder_ident(&source);
            assert!(result.is_err());
        }
    })
}

fn rust_type(field_type: &FieldType) -> Result<TokenStream> {
    let field_type = syn::parse_str::<syn::Type>(field_type.rust_type())
        .context("解析生成字段 Rust 类型失败")?;
    Ok(quote! { #field_type })
}

fn type_ident(code: &str) -> Result<Ident> {
    let candidate = code.to_case(Case::Pascal);
    syn::parse_str::<Ident>(&candidate)
        .with_context(|| format!("无法把 {code} 生成为 Rust 类型标识符"))
}

fn value_ident(code: &str) -> Result<Ident> {
    let candidate = code.to_case(Case::Snake);
    syn::parse_str::<Ident>(&candidate)
        .with_context(|| format!("无法把 {code} 生成为 Rust 值标识符"))
}

fn format_tokens(tokens: TokenStream) -> Result<String> {
    let syntax = syn::parse2(tokens).context("解析生成 Rust 源码失败")?;
    Ok(prettyplease::unparse(&syntax))
}
