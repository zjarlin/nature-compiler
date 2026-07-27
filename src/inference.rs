use std::collections::BTreeMap;

use anyhow::{Result, bail};
use async_trait::async_trait;
use deunicode::deunicode;

use crate::{
    ApplicationDefinition, AppliedDefault, Blueprint, CapabilityRequirement, CompilerCatalog,
    DescriptorEncoder, Diagnostic, DomainDefinition, DomainMetadata, DomainOperation, Encode,
    FieldBinding, FieldDefinition, FieldType, FunctionDefinition, HttpMethod, InferenceDecision,
    InferenceMetrics, InterfaceDefinition, LogicStep, NavigationDefinition, NavigationEntry,
    OperationIntent, OperationPlanStep, PermissionDefinition, PermissionRule, Requirement,
    SemanticDescriptor, StructDefinition, ValidationRule, ValueTransform, ViewActionDefinition,
    ViewDefinition, ViewLayout, normalize_inferred_stem,
};

/// 推导阶段的强类型结果。
#[derive(Clone, Debug, PartialEq)]
pub struct InferenceResult {
    pub blueprint: Blueprint,
    pub diagnostics: Vec<Diagnostic>,
    pub metrics: InferenceMetrics,
}

/// AI 或确定性规则引擎必须实现的受控推导边界。
#[async_trait]
pub trait InferenceEngine: Send + Sync {
    async fn infer(
        &self,
        source_text: &str,
        previous_blueprint: Option<&Blueprint>,
        catalog: &CompilerCatalog,
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
        _catalog: &CompilerCatalog,
    ) -> Result<InferenceResult> {
        infer_chinese_blueprint(source_text, previous_blueprint)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceSection {
    None,
    Requirements,
    Modeling,
    Acquisition,
    Operations,
    View(usize),
    Navigation,
    Permissions,
}

#[derive(Clone, Debug)]
struct ParsedField {
    native_name: String,
    field_type: FieldType,
    required: bool,
    unique: bool,
    email: bool,
    unit: Option<String>,
    range: Option<(f64, f64)>,
    type_was_defaulted: bool,
}

#[derive(Clone, Debug)]
struct ParsedView {
    native_name: String,
    lines: Vec<String>,
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
    let mut operation_lines = Vec::new();
    let mut parsed_views = Vec::new();
    let mut navigation_lines = Vec::new();
    let mut permission_lines = Vec::new();
    let mut domain_native_name = None;
    let mut model_native_name = None;

    for line in source_text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(value) = section_title(line, "领域") {
            if !value.is_empty() {
                domain_native_name = Some(value.to_string());
            }
            section = SourceSection::None;
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
        if section_title(line, "操作").is_some() {
            section = SourceSection::Operations;
            continue;
        }
        if let Some(value) = section_title(line, "界面") {
            if !value.is_empty() {
                parsed_views.push(ParsedView {
                    native_name: value.to_string(),
                    lines: Vec::new(),
                });
                section = SourceSection::View(parsed_views.len() - 1);
            }
            continue;
        }
        if section_title(line, "导航").is_some() {
            section = SourceSection::Navigation;
            continue;
        }
        if section_title(line, "权限").is_some() {
            section = SourceSection::Permissions;
            continue;
        }

        let content = strip_list_marker(line);
        match &section {
            SourceSection::Requirements => requirements.push(content.to_string()),
            SourceSection::Modeling => parsed_fields.push(parse_field(content)),
            SourceSection::Acquisition => acquisition_lines.push(content.to_string()),
            SourceSection::Operations => operation_lines.push(content.to_string()),
            SourceSection::View(index) => parsed_views[*index].lines.push(content.to_string()),
            SourceSection::Navigation => navigation_lines.push(content.to_string()),
            SourceSection::Permissions => permission_lines.push(content.to_string()),
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
    let domain_native_name =
        domain_native_name.unwrap_or_else(|| format!("{model_native_name}领域"));
    let domain_descriptor = factory.describe(&domain_native_name, None);
    let strict_required = requirements
        .iter()
        .any(|text| text.contains("无效数据不能入库"));
    let mut diagnostics = Vec::new();
    let mut defaults = Vec::new();
    let mut fields = Vec::new();
    for parsed in parsed_fields {
        let required = parsed.required || strict_required;
        let descriptor = factory.describe(&parsed.native_name, None);
        let mut validations = Vec::new();
        if required {
            validations.push(ValidationRule::Required);
        }
        if parsed.unique {
            validations.push(ValidationRule::Unique);
        }
        if parsed.email {
            validations.push(ValidationRule::Email);
        }
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
        if required {
            defaults.push(AppliedDefault {
                subject: parsed.native_name.clone(),
                value: "必填".to_string(),
                reason: "母语字段明确声明必填，或需求要求无效数据不得入库".to_string(),
            });
        }
        let mut domain_metadata = Vec::new();
        if let Some(unit) = parsed.unit.as_deref() {
            domain_metadata.push(DomainMetadata {
                namespace: "measurement".to_string(),
                name: "单位".to_string(),
                value: unit.to_string(),
            });
        }
        match parsed.field_type {
            FieldType::Password => domain_metadata.push(DomainMetadata {
                namespace: "security".to_string(),
                name: "输入语义".to_string(),
                value: "密码".to_string(),
            }),
            FieldType::Dictionary => domain_metadata.push(DomainMetadata {
                namespace: "dictionary".to_string(),
                name: "母语字典".to_string(),
                value: parsed.native_name.clone(),
            }),
            _ => {}
        }
        fields.push(FieldDefinition {
            descriptor,
            field_type: parsed.field_type,
            required,
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
    let (capabilities, bindings) = if acquisition_lines.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        infer_acquisition(
            &model_descriptor,
            &fields,
            &acquisition_lines,
            &mut factory,
            &mut diagnostics,
        )
    };
    let functions = if acquisition_lines.is_empty() {
        Vec::new()
    } else {
        let function_native_name = format!("获取并校验{model_native_name}");
        let function_descriptor =
            factory.describe(&function_native_name, Some("process_telemetry"));
        let input_descriptor = factory.describe("原始数据", Some("source_values"));
        vec![FunctionDefinition {
            descriptor: function_descriptor,
            input_model: input_descriptor,
            output_model: model_descriptor.clone(),
            steps: vec![
                LogicStep::ReadSourceMap,
                LogicStep::DecodeBindings,
                LogicStep::ValidateFields,
                LogicStep::ReturnAcceptedValue,
            ],
        }]
    };
    let application = infer_application(
        domain_descriptor,
        model_descriptor.clone(),
        &fields,
        &operation_lines,
        &parsed_views,
        &navigation_lines,
        &permission_lines,
        &mut factory,
    );

    let reused_semantics = factory.reused_semantics;
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
        application,
        capabilities,
        bindings,
        inference_decisions,
        defaults,
    };
    let mut metrics = InferenceMetrics::deterministic("mother_tongue");
    metrics.reused_semantics = reused_semantics;
    Ok(InferenceResult {
        blueprint,
        diagnostics,
        metrics,
    })
}

#[allow(clippy::too_many_arguments)]
fn infer_application(
    domain_descriptor: SemanticDescriptor,
    model_descriptor: SemanticDescriptor,
    fields: &[FieldDefinition],
    operation_lines: &[String],
    parsed_views: &[ParsedView],
    navigation_lines: &[String],
    permission_lines: &[String],
    factory: &mut DescriptorFactory<'_>,
) -> ApplicationDefinition {
    let mut operations = default_crud_operations(&model_descriptor, factory);
    operations.extend(operation_lines.iter().map(|line| {
        let intent = infer_operation_intent(line);
        DomainOperation {
            descriptor: factory.describe(line, None),
            model: model_descriptor.clone(),
            intent,
            steps: operation_steps(intent),
        }
    }));
    let interfaces = operations
        .iter()
        .map(|operation| InterfaceDefinition {
            descriptor: operation.descriptor.clone(),
            operation: operation.descriptor.clone(),
            method: operation_method(operation.intent),
            path: operation_path(&domain_descriptor, &model_descriptor, operation),
        })
        .collect::<Vec<_>>();
    let views = infer_views(
        &domain_descriptor,
        &model_descriptor,
        fields,
        &operations,
        parsed_views,
        factory,
    );
    let permissions = infer_permissions(permission_lines, &operations, factory);
    let navigation = infer_navigation(&domain_descriptor, &views, &permissions, navigation_lines);
    ApplicationDefinition {
        domain: DomainDefinition {
            descriptor: domain_descriptor,
            models: vec![model_descriptor],
        },
        operations,
        interfaces,
        views,
        navigation,
        permissions,
    }
}

fn default_crud_operations(
    model: &SemanticDescriptor,
    factory: &mut DescriptorFactory<'_>,
) -> Vec<DomainOperation> {
    [
        (
            format!("查询{}列表", model.native_name),
            OperationIntent::List,
        ),
        (format!("查看{}", model.native_name), OperationIntent::Read),
        (
            format!("新增{}", model.native_name),
            OperationIntent::Create,
        ),
        (
            format!("修改{}", model.native_name),
            OperationIntent::Update,
        ),
        (
            format!("删除{}", model.native_name),
            OperationIntent::Delete,
        ),
    ]
    .into_iter()
    .map(|(native_name, intent)| DomainOperation {
        descriptor: factory.describe(&native_name, None),
        model: model.clone(),
        intent,
        steps: operation_steps(intent),
    })
    .collect()
}

fn infer_operation_intent(value: &str) -> OperationIntent {
    if value.contains("登录") || value.contains("认证") {
        OperationIntent::Authenticate
    } else if value.contains("删除") {
        OperationIntent::Delete
    } else if value.contains("修改") || value.contains("更新") || value.contains("停用") {
        OperationIntent::Update
    } else if value.contains("新增")
        || value.contains("注册")
        || value.contains("创建")
        || value.contains("保存")
    {
        OperationIntent::Create
    } else if value.contains("查看") || value.contains("详情") {
        OperationIntent::Read
    } else if value.contains("查询") || value.contains("筛选") || value.contains("列表") {
        OperationIntent::List
    } else {
        OperationIntent::Command
    }
}

fn operation_steps(intent: OperationIntent) -> Vec<OperationPlanStep> {
    match intent {
        OperationIntent::List => vec![
            OperationPlanStep::QueryRecords,
            OperationPlanStep::ReturnResult,
        ],
        OperationIntent::Read => vec![
            OperationPlanStep::LoadRecord,
            OperationPlanStep::ReturnResult,
        ],
        OperationIntent::Create => vec![
            OperationPlanStep::ValidateInput,
            OperationPlanStep::CreateRecord,
            OperationPlanStep::ReturnResult,
        ],
        OperationIntent::Update => vec![
            OperationPlanStep::ValidateInput,
            OperationPlanStep::LoadRecord,
            OperationPlanStep::UpdateRecord,
            OperationPlanStep::ReturnResult,
        ],
        OperationIntent::Delete => vec![
            OperationPlanStep::LoadRecord,
            OperationPlanStep::DeleteRecord,
            OperationPlanStep::ReturnResult,
        ],
        OperationIntent::Authenticate | OperationIntent::Command => vec![
            OperationPlanStep::ValidateInput,
            OperationPlanStep::QueryRecords,
            OperationPlanStep::ReturnResult,
        ],
    }
}

fn operation_method(intent: OperationIntent) -> HttpMethod {
    match intent {
        OperationIntent::List | OperationIntent::Read => HttpMethod::Get,
        OperationIntent::Create | OperationIntent::Authenticate | OperationIntent::Command => {
            HttpMethod::Post
        }
        OperationIntent::Update => HttpMethod::Put,
        OperationIntent::Delete => HttpMethod::Delete,
    }
}

fn operation_path(
    domain: &SemanticDescriptor,
    model: &SemanticDescriptor,
    operation: &DomainOperation,
) -> String {
    let base = format!("/api/app/{}/{}", domain.encode(), model.encode());
    let default_name = match operation.intent {
        OperationIntent::List => format!("查询{}列表", model.native_name),
        OperationIntent::Read => format!("查看{}", model.native_name),
        OperationIntent::Create => format!("新增{}", model.native_name),
        OperationIntent::Update => format!("修改{}", model.native_name),
        OperationIntent::Delete => format!("删除{}", model.native_name),
        OperationIntent::Authenticate | OperationIntent::Command => String::new(),
    };
    if operation.descriptor.native_name != default_name && !default_name.is_empty() {
        return match operation.intent {
            OperationIntent::List | OperationIntent::Create => {
                format!("{base}/{}", operation.encode())
            }
            OperationIntent::Read | OperationIntent::Update | OperationIntent::Delete => {
                format!("{base}/{{id}}/{}", operation.encode())
            }
            OperationIntent::Authenticate | OperationIntent::Command => base,
        };
    }
    match operation.intent {
        OperationIntent::List | OperationIntent::Create => base,
        OperationIntent::Read | OperationIntent::Update | OperationIntent::Delete => {
            format!("{base}/{{id}}")
        }
        OperationIntent::Authenticate => format!("{base}/authenticate"),
        OperationIntent::Command => format!("{base}/{}", operation.encode()),
    }
}

fn infer_views(
    domain: &SemanticDescriptor,
    model: &SemanticDescriptor,
    fields: &[FieldDefinition],
    operations: &[DomainOperation],
    parsed_views: &[ParsedView],
    factory: &mut DescriptorFactory<'_>,
) -> Vec<ViewDefinition> {
    if parsed_views.is_empty() {
        return [
            (format!("{}列表", model.native_name), ViewLayout::Table),
            (format!("{}详情", model.native_name), ViewLayout::Detail),
            (format!("{}表单", model.native_name), ViewLayout::Form),
        ]
        .into_iter()
        .map(|(name, layout)| {
            build_view(
                domain,
                model,
                fields,
                operations,
                &name,
                layout,
                &[],
                factory,
            )
        })
        .collect();
    }
    parsed_views
        .iter()
        .map(|view| {
            let layout = if view.native_name.contains("列表")
                || view.lines.iter().any(|line| line.contains("表格"))
            {
                ViewLayout::Table
            } else if view.native_name.contains("资料")
                || view.native_name.contains("表单")
                || view.lines.iter().any(|line| line.contains("表单"))
            {
                ViewLayout::Form
            } else {
                ViewLayout::Detail
            };
            build_view(
                domain,
                model,
                fields,
                operations,
                &view.native_name,
                layout,
                &view.lines,
                factory,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_view(
    domain: &SemanticDescriptor,
    model: &SemanticDescriptor,
    fields: &[FieldDefinition],
    operations: &[DomainOperation],
    native_name: &str,
    layout: ViewLayout,
    lines: &[String],
    factory: &mut DescriptorFactory<'_>,
) -> ViewDefinition {
    let descriptor = factory.describe(native_name, None);
    let selected_fields = fields
        .iter()
        .filter(|field| {
            lines.is_empty()
                || lines
                    .iter()
                    .any(|line| line.contains(&field.descriptor.native_name))
        })
        .map(|field| field.descriptor.clone())
        .collect::<Vec<_>>();
    let fields = if selected_fields.is_empty() {
        fields
            .iter()
            .map(|field| field.descriptor.clone())
            .collect()
    } else {
        selected_fields
    };
    let actions = operations
        .iter()
        .filter(|operation| {
            lines
                .iter()
                .any(|line| line.contains(&operation.descriptor.native_name))
                || matches!(
                    (layout, operation.intent),
                    (ViewLayout::Table, OperationIntent::Create)
                        | (ViewLayout::Form, OperationIntent::Update)
                )
        })
        .map(|operation| ViewActionDefinition {
            descriptor: operation.descriptor.clone(),
            operation: operation.descriptor.clone(),
        })
        .collect();
    ViewDefinition {
        route: format!("/{}/{}", domain.encode(), descriptor.encode()),
        descriptor,
        model: model.clone(),
        layout,
        fields,
        actions,
    }
}

fn infer_permissions(
    lines: &[String],
    operations: &[DomainOperation],
    factory: &mut DescriptorFactory<'_>,
) -> Vec<PermissionDefinition> {
    lines
        .iter()
        .map(|line| {
            let actor = line
                .split_once("可以")
                .or_else(|| line.split_once("只能"))
                .map(|(actor, _)| actor.trim())
                .filter(|actor| !actor.is_empty())
                .unwrap_or(line);
            let rule = if line.contains("自己") {
                PermissionRule::OwnRecords
            } else if line.contains("全部") || line.contains("管理员") {
                PermissionRule::AllRecords
            } else {
                PermissionRule::Authenticated
            };
            let allowed_operations = operations
                .iter()
                .filter(|operation| {
                    rule == PermissionRule::AllRecords
                        || !matches!(
                            operation.intent,
                            OperationIntent::List | OperationIntent::Delete
                        )
                })
                .map(|operation| operation.descriptor.clone())
                .collect();
            PermissionDefinition {
                descriptor: factory.describe(actor, None),
                rule,
                operations: allowed_operations,
            }
        })
        .collect()
}

fn infer_navigation(
    domain: &SemanticDescriptor,
    views: &[ViewDefinition],
    permissions: &[PermissionDefinition],
    lines: &[String],
) -> NavigationDefinition {
    let section_label = lines
        .iter()
        .find_map(|line| quoted_text(line))
        .unwrap_or_else(|| "业务".to_string());
    let default_view = views
        .iter()
        .find(|view| {
            lines
                .iter()
                .any(|line| line.contains("默认") && line.contains(&view.descriptor.native_name))
        })
        .unwrap_or(&views[0])
        .descriptor
        .clone();
    let entries = views
        .iter()
        .enumerate()
        .map(|(index, view)| {
            let permissions = permissions
                .iter()
                .filter(|permission| {
                    view.layout != ViewLayout::Table
                        || permission.rule == PermissionRule::AllRecords
                })
                .map(|permission| permission.descriptor.clone())
                .collect();
            NavigationEntry {
                descriptor: view.descriptor.clone(),
                view: view.descriptor.clone(),
                order: i32::try_from(index + 1).unwrap_or(i32::MAX) * 10,
                permissions,
            }
        })
        .collect();
    NavigationDefinition {
        section_label,
        descriptor: domain.clone(),
        default_view,
        entries,
    }
}

fn quoted_text(value: &str) -> Option<String> {
    let (_, suffix) = value.split_once(['“', '"'])?;
    let (quoted, _) = suffix.split_once(['”', '"'])?;
    (!quoted.trim().is_empty()).then(|| quoted.trim().to_string())
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
    let field_type = if detail.contains("密码") {
        FieldType::Password
    } else if detail.contains("邮箱") {
        FieldType::Email
    } else if detail.contains("字典") {
        FieldType::Dictionary
    } else if detail.contains("小数") || detail.contains("百分比") {
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
            "密码",
            "邮箱",
            "字典",
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
        required: detail.contains("必填"),
        unique: detail.contains("唯一"),
        email: detail.contains("邮箱"),
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
    previous_decisions: BTreeMap<String, InferenceDecision>,
    encoder: DescriptorEncoder,
    decisions: Vec<InferenceDecision>,
    reused_semantics: u64,
    _previous_blueprint: Option<&'a Blueprint>,
}

impl<'a> DescriptorFactory<'a> {
    fn new(previous_blueprint: Option<&'a Blueprint>) -> Self {
        let mut previous = BTreeMap::new();
        let mut previous_decisions = BTreeMap::new();
        if let Some(blueprint) = previous_blueprint {
            collect_descriptors(blueprint, &mut previous);
            previous_decisions.extend(
                blueprint
                    .inference_decisions
                    .iter()
                    .cloned()
                    .map(|decision| (decision.subject.clone(), decision)),
            );
        }
        Self {
            previous,
            previous_decisions,
            encoder: DescriptorEncoder::default(),
            decisions: Vec::new(),
            reused_semantics: 0,
            _previous_blueprint: previous_blueprint,
        }
    }

    fn describe(&mut self, native_name: &str, preferred_stem: Option<&str>) -> SemanticDescriptor {
        if let Some(previous) = self.previous.get(native_name).cloned() {
            self.encoder.reserve(&previous);
            self.reused_semantics += 1;
            let decision = self
                .previous_decisions
                .get(native_name)
                .cloned()
                .unwrap_or_else(|| InferenceDecision {
                    subject: native_name.to_string(),
                    decision: format!(
                        "推导英文语义 {}，编码为 {}",
                        previous.english_stem, previous.code
                    ),
                    reused: false,
                });
            self.decisions.push(decision);
            return previous;
        }

        let english_stem = preferred_stem
            .map(str::to_string)
            .unwrap_or_else(|| translate_semantic_name(native_name));
        let descriptor = self.encoder.describe(native_name, &english_stem);
        let code = descriptor.code.clone();
        self.decisions.push(InferenceDecision {
            subject: native_name.to_string(),
            decision: format!("推导英文语义 {english_stem}，编码为 {code}"),
            reused: false,
        });
        descriptor
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
    target.insert(
        blueprint.application.domain.descriptor.native_name.clone(),
        blueprint.application.domain.descriptor.clone(),
    );
    for operation in &blueprint.application.operations {
        target.insert(
            operation.descriptor.native_name.clone(),
            operation.descriptor.clone(),
        );
    }
    for view in &blueprint.application.views {
        target.insert(view.descriptor.native_name.clone(), view.descriptor.clone());
    }
    for permission in &blueprint.application.permissions {
        target.insert(
            permission.descriptor.native_name.clone(),
            permission.descriptor.clone(),
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
    normalize_inferred_stem(&deunicode(native_name), native_name)
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
