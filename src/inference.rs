use std::collections::BTreeMap;

use anyhow::{Result, bail};
use async_trait::async_trait;
use convert_case::{Case, Casing};
use deunicode::deunicode;
use sha2::{Digest, Sha256};

use crate::{
    AppliedDefault, Blueprint, CapabilityRequirement, Diagnostic, DomainMetadata, Encode,
    FieldBinding, FieldDefinition, FieldType, FunctionDefinition, InferenceDecision, LogicStep,
    Requirement, SemanticDescriptor, StructDefinition, ValidationRule, ValueTransform,
};

/// 推导阶段的强类型结果。
#[derive(Clone, Debug, PartialEq)]
pub struct InferenceResult {
    pub blueprint: Blueprint,
    pub diagnostics: Vec<Diagnostic>,
}

/// AI 或确定性规则引擎必须实现的受控推导边界。
#[async_trait]
pub trait InferenceEngine: Send + Sync {
    async fn infer(
        &self,
        source_text: &str,
        previous_blueprint: Option<&Blueprint>,
    ) -> Result<InferenceResult>;
}

/// 中文首版的确定性推导器，可作为 Agent 不可用时的正式降级实现。
#[derive(Clone, Debug, Default)]
pub struct MotherTongueInferenceEngine;

#[async_trait]
impl InferenceEngine for MotherTongueInferenceEngine {
    async fn infer(
        &self,
        source_text: &str,
        previous_blueprint: Option<&Blueprint>,
    ) -> Result<InferenceResult> {
        infer_chinese_blueprint(source_text, previous_blueprint)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceSection {
    None,
    Requirements,
    Modeling,
    Acquisition,
}

#[derive(Clone, Debug)]
struct ParsedField {
    native_name: String,
    field_type: FieldType,
    unit: Option<String>,
    range: Option<(f64, f64)>,
    type_was_defaulted: bool,
}

fn infer_chinese_blueprint(
    source_text: &str,
    previous_blueprint: Option<&Blueprint>,
) -> Result<InferenceResult> {
    let source_text = source_text.trim();
    if source_text.is_empty() {
        bail!("母语需求不能为空");
    }

    let mut factory = DescriptorFactory::new(previous_blueprint);
    let mut section = SourceSection::None;
    let mut requirements = Vec::new();
    let mut parsed_fields = Vec::new();
    let mut acquisition_lines = Vec::new();
    let mut model_native_name = None;

    for line in source_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = section_title(line, "需求") {
            section = SourceSection::Requirements;
            if !value.is_empty() {
                requirements.push(value.to_string());
            }
            continue;
        }
        if let Some(value) = section_title(line, "建模") {
            section = SourceSection::Modeling;
            if !value.is_empty() {
                model_native_name = Some(value.to_string());
            }
            continue;
        }
        if section_title(line, "数据获取").is_some() {
            section = SourceSection::Acquisition;
            continue;
        }

        let content = strip_list_marker(line);
        match section {
            SourceSection::Requirements => requirements.push(content.to_string()),
            SourceSection::Modeling => parsed_fields.push(parse_field(content)),
            SourceSection::Acquisition => acquisition_lines.push(content.to_string()),
            SourceSection::None => {}
        }
    }

    let Some(model_native_name) = model_native_name else {
        bail!("建模章节必须使用“建模：名称”声明领域结构");
    };
    if parsed_fields.is_empty() {
        bail!("建模章节至少需要一个母语字段");
    }

    let model_descriptor = factory.describe(&model_native_name, None);
    let mut diagnostics = Vec::new();
    let mut defaults = Vec::new();
    let mut fields = Vec::new();
    for parsed in parsed_fields {
        let descriptor = factory.describe(&parsed.native_name, None);
        let mut validations = vec![ValidationRule::Required];
        if let Some((minimum, maximum)) = parsed.range {
            validations.push(ValidationRule::NumberRange { minimum, maximum });
        }
        if parsed.type_was_defaulted {
            diagnostics.push(Diagnostic::warning(
                &parsed.native_name,
                "没有识别到明确类型，已按文本处理；发布前应确认该默认值。",
            ));
            defaults.push(AppliedDefault {
                subject: parsed.native_name.clone(),
                value: "文本".to_string(),
                reason: "母语定义没有提供可识别的字段类型".to_string(),
            });
        }
        defaults.push(AppliedDefault {
            subject: parsed.native_name.clone(),
            value: "必填".to_string(),
            reason: "无效数据不能入库，因此首版字段采用必填安全默认".to_string(),
        });
        let mut domain_metadata = Vec::new();
        if let Some(unit) = parsed.unit.as_deref() {
            domain_metadata.push(DomainMetadata {
                namespace: "measurement".to_string(),
                name: "单位".to_string(),
                value: unit.to_string(),
            });
        }
        fields.push(FieldDefinition {
            descriptor,
            field_type: parsed.field_type,
            required: true,
            unit: parsed.unit,
            validations,
            domain_metadata,
        });
    }

    let requirements = requirements
        .into_iter()
        .map(|text| Requirement {
            descriptor: factory.describe(&text, None),
            text,
        })
        .collect::<Vec<_>>();
    let (capabilities, bindings) = infer_acquisition(
        &model_descriptor,
        &fields,
        &acquisition_lines,
        &mut factory,
        &mut diagnostics,
    );
    let function_native_name = format!("获取并校验{model_native_name}");
    let function_descriptor = factory.describe(&function_native_name, Some("process_telemetry"));
    let input_descriptor = factory.describe("原始数据", Some("source_values"));
    let functions = vec![FunctionDefinition {
        descriptor: function_descriptor,
        input_model: input_descriptor,
        output_model: model_descriptor.clone(),
        steps: vec![
            LogicStep::ReadSourceMap,
            LogicStep::DecodeBindings,
            LogicStep::ValidateFields,
            LogicStep::ReturnAcceptedValue,
        ],
    }];

    let inference_decisions = factory.decisions;
    let blueprint = Blueprint {
        source_text: source_text.to_string(),
        requirements,
        structs: vec![StructDefinition {
            descriptor: model_descriptor,
            fields,
        }],
        enums: Vec::new(),
        functions,
        capabilities,
        bindings,
        inference_decisions,
        defaults,
    };
    Ok(InferenceResult {
        blueprint,
        diagnostics,
    })
}

fn infer_acquisition(
    owner: &SemanticDescriptor,
    fields: &[FieldDefinition],
    lines: &[String],
    factory: &mut DescriptorFactory<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<CapabilityRequirement>, Vec<FieldBinding>) {
    let mut capabilities = Vec::new();
    if let Some(line) = lines.iter().find(|line| line.contains("模拟采集")) {
        capabilities.push(CapabilityRequirement {
            descriptor: factory.describe("模拟采集", Some("fixture_map")),
            source_phrase: line.clone(),
        });
    }

    let mut bindings = Vec::new();
    for field in fields {
        let matching_line = lines
            .iter()
            .find(|line| line.starts_with(&field.descriptor.native_name) && line.contains("原值"));
        let Some(line) = matching_line else {
            diagnostics.push(Diagnostic::error(
                &field.descriptor.native_name,
                "没有找到字段对应的数据获取映射。",
            ));
            continue;
        };
        let divisor = number_after(line, "除以").unwrap_or(1.0);
        let source_native = format!("{}原值", field.descriptor.native_name);
        let source_stem = fixture_source_stem(field.encode(), divisor);
        bindings.push(FieldBinding {
            owner: owner.clone(),
            field: field.descriptor.clone(),
            source: factory.describe(&source_native, Some(&source_stem)),
            transform: if (divisor - 1.0).abs() < f64::EPSILON {
                ValueTransform::Identity
            } else {
                ValueTransform::Divide { divisor }
            },
        });
    }

    if capabilities.is_empty() {
        diagnostics.push(Diagnostic::error(
            "数据获取",
            "没有找到可解析的数据源能力；请用母语说明数据来自哪里。",
        ));
    }
    (capabilities, bindings)
}

fn fixture_source_stem(field_code: &str, divisor: f64) -> String {
    let prefix = match field_code {
        "temperature" => "temp",
        other => other,
    };
    if divisor.fract().abs() < f64::EPSILON && divisor > 1.0 {
        format!("{prefix}_x{}", divisor as i64)
    } else {
        format!("{prefix}_raw")
    }
}

fn parse_field(line: &str) -> ParsedField {
    let (native_name, detail) = line
        .split_once('：')
        .or_else(|| line.split_once(':'))
        .unwrap_or((line, ""));
    let field_type = if detail.contains("小数") || detail.contains("百分比") {
        FieldType::Decimal
    } else if detail.contains("整数") {
        FieldType::Integer
    } else if detail.contains("布尔") || detail.contains("是否") {
        FieldType::Boolean
    } else if detail.contains("时间") {
        FieldType::Timestamp
    } else if detail.contains("JSON") {
        FieldType::Json
    } else {
        FieldType::String
    };
    let type_was_defaulted = detail.is_empty()
        || ![
            "小数",
            "百分比",
            "整数",
            "布尔",
            "是否",
            "时间",
            "JSON",
            "文本",
        ]
        .iter()
        .any(|marker| detail.contains(marker));
    let unit = detail
        .split(['，', ','])
        .map(str::trim)
        .find(|part| ["摄氏度", "百分比", "伏", "安", "帕", "米"].contains(part))
        .map(str::to_string);
    let range = if detail.contains("范围") {
        let numbers = extract_numbers(detail);
        match numbers.as_slice() {
            [minimum, maximum, ..] => Some((*minimum, *maximum)),
            _ => None,
        }
    } else {
        None
    };
    ParsedField {
        native_name: native_name.trim().to_string(),
        field_type,
        unit,
        range,
        type_was_defaulted,
    }
}

fn extract_numbers(value: &str) -> Vec<f64> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        if character.is_ascii_digit() || character == '-' || character == '.' {
            current.push(character);
            continue;
        }
        if !current.is_empty() {
            if let Ok(number) = current.parse::<f64>() {
                numbers.push(number);
            }
            current.clear();
        }
    }
    if let Ok(number) = current.parse::<f64>() {
        numbers.push(number);
    }
    numbers
}

fn number_after(value: &str, marker: &str) -> Option<f64> {
    let (_, suffix) = value.split_once(marker)?;
    extract_numbers(suffix).into_iter().next()
}

fn section_title<'a>(line: &'a str, title: &str) -> Option<&'a str> {
    let suffix = line.strip_prefix(title)?;
    Some(suffix.trim_start_matches(['：', ':']).trim())
}

fn strip_list_marker(line: &str) -> &str {
    let line = line.trim_start();
    let digit_count = line.chars().take_while(char::is_ascii_digit).count();
    let suffix = &line[digit_count..];
    suffix
        .strip_prefix('.')
        .or_else(|| suffix.strip_prefix('、'))
        .unwrap_or(suffix)
        .trim()
}

struct DescriptorFactory<'a> {
    previous: BTreeMap<String, SemanticDescriptor>,
    claimed_codes: BTreeMap<String, String>,
    decisions: Vec<InferenceDecision>,
    _previous_blueprint: Option<&'a Blueprint>,
}

impl<'a> DescriptorFactory<'a> {
    fn new(previous_blueprint: Option<&'a Blueprint>) -> Self {
        let mut previous = BTreeMap::new();
        if let Some(blueprint) = previous_blueprint {
            collect_descriptors(blueprint, &mut previous);
        }
        Self {
            previous,
            claimed_codes: BTreeMap::new(),
            decisions: Vec::new(),
            _previous_blueprint: previous_blueprint,
        }
    }

    fn describe(&mut self, native_name: &str, preferred_stem: Option<&str>) -> SemanticDescriptor {
        if let Some(previous) = self.previous.get(native_name).cloned() {
            self.claimed_codes
                .insert(previous.code.clone(), native_name.to_string());
            self.decisions.push(InferenceDecision {
                subject: native_name.to_string(),
                decision: format!("复用英文语义 {}", previous.english_stem),
                reused: true,
            });
            return previous;
        }

        let english_stem = preferred_stem
            .map(str::to_string)
            .unwrap_or_else(|| translate_semantic_name(native_name));
        let base_code = normalize_code(&english_stem);
        let code = match self.claimed_codes.get(&base_code) {
            Some(claimed_by) if claimed_by != native_name => {
                format!("{base_code}_{}", short_hash(native_name))
            }
            _ => base_code,
        };
        self.claimed_codes
            .insert(code.clone(), native_name.to_string());
        self.decisions.push(InferenceDecision {
            subject: native_name.to_string(),
            decision: format!("推导英文语义 {english_stem}，编码为 {code}"),
            reused: false,
        });
        SemanticDescriptor::new(native_name, english_stem, code)
    }
}

fn collect_descriptors(blueprint: &Blueprint, target: &mut BTreeMap<String, SemanticDescriptor>) {
    for requirement in &blueprint.requirements {
        target.insert(
            requirement.descriptor.native_name.clone(),
            requirement.descriptor.clone(),
        );
    }
    for definition in &blueprint.structs {
        target.insert(
            definition.descriptor.native_name.clone(),
            definition.descriptor.clone(),
        );
        for field in &definition.fields {
            target.insert(
                field.descriptor.native_name.clone(),
                field.descriptor.clone(),
            );
        }
    }
    for function in &blueprint.functions {
        target.insert(
            function.descriptor.native_name.clone(),
            function.descriptor.clone(),
        );
    }
    for capability in &blueprint.capabilities {
        target.insert(
            capability.descriptor.native_name.clone(),
            capability.descriptor.clone(),
        );
    }
    for binding in &blueprint.bindings {
        target.insert(binding.source.native_name.clone(), binding.source.clone());
    }
}

fn translate_semantic_name(native_name: &str) -> String {
    let exact = BTreeMap::from([
        ("环境遥测", "environment_telemetry"),
        ("环境采集", "environment_collection"),
        ("温度", "temperature"),
        ("湿度", "humidity"),
        ("姓名", "name"),
        ("年龄", "age"),
        ("性别", "gender"),
        ("用户", "user"),
        ("模拟采集", "fixture_map"),
    ]);
    if let Some(value) = exact.get(native_name) {
        return (*value).to_string();
    }
    normalize_code(&deunicode(native_name))
}

fn normalize_code(value: &str) -> String {
    let already_snake = value.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
    });
    let normalized = if already_snake {
        value.to_string()
    } else {
        value.to_case(Case::Snake)
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
    let segments = filtered
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let candidate = segments.join("_");
    if candidate.is_empty() {
        format!("semantic_{}", short_hash(value))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_negative_decimal_range() {
        assert_eq!(extract_numbers("范围 -40 到 125"), vec![-40.0, 125.0]);
    }

    #[test]
    fn collision_adds_stable_short_hash() {
        let mut factory = DescriptorFactory::new(None);
        let first = factory.describe("甲", Some("same"));
        let second = factory.describe("乙", Some("same"));

        assert_eq!(first.code, "same");
        assert!(second.code.starts_with("same_"));
        assert_eq!(second.code.len(), 13);
    }

    #[test]
    fn list_marker_is_removed_without_touching_content() {
        assert_eq!(strip_list_marker("12. 温度：小数"), "温度：小数");
    }
}
