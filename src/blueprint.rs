use serde::{Deserialize, Serialize};

use crate::{Encode, SemanticDescriptor};

/// 编译器的唯一中间表示，所有后端只消费该结构。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    pub source_text: String,
    pub requirements: Vec<Requirement>,
    pub structs: Vec<StructDefinition>,
    pub enums: Vec<EnumDefinition>,
    pub functions: Vec<FunctionDefinition>,
    pub application: ApplicationDefinition,
    pub capabilities: Vec<CapabilityRequirement>,
    pub bindings: Vec<FieldBinding>,
    pub inference_decisions: Vec<InferenceDecision>,
    pub defaults: Vec<AppliedDefault>,
}

/// 单条母语需求。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    pub descriptor: SemanticDescriptor,
    pub text: String,
}

impl Encode for Requirement {
    fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 结构体定义。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructDefinition {
    pub descriptor: SemanticDescriptor,
    pub fields: Vec<FieldDefinition>,
}

impl StructDefinition {
    pub fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 结构体字段。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDefinition {
    pub descriptor: SemanticDescriptor,
    pub field_type: FieldType,
    pub required: bool,
    pub unit: Option<String>,
    pub validations: Vec<ValidationRule>,
    pub domain_metadata: Vec<DomainMetadata>,
}

impl Encode for FieldDefinition {
    fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 编译器支持的领域字段类型。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Password,
    Email,
    Dictionary,
    Integer,
    Decimal,
    Boolean,
    Timestamp,
    Json,
}

impl FieldType {
    pub fn rust_type(&self) -> &'static str {
        match self {
            Self::String | Self::Password | Self::Email | Self::Dictionary => "String",
            Self::Integer | Self::Timestamp => "i64",
            Self::Decimal => "f64",
            Self::Boolean => "bool",
            Self::Json => "serde_json::Value",
        }
    }
}

/// 字段校验约束。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationRule {
    Required,
    Unique,
    Email,
    NumberRange { minimum: f64, maximum: f64 },
}

/// 一个母语项目对应的完整应用定义。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDefinition {
    pub domain: DomainDefinition,
    pub operations: Vec<DomainOperation>,
    pub interfaces: Vec<InterfaceDefinition>,
    pub views: Vec<ViewDefinition>,
    pub navigation: NavigationDefinition,
    pub permissions: Vec<PermissionDefinition>,
}

impl ApplicationDefinition {
    /// 语义 code 变化后重新计算全部领域路由。
    pub fn refresh_derived_paths(&mut self) {
        let domain_code = self.domain.descriptor.encode().to_string();
        for interface in &mut self.interfaces {
            let Some(operation) = self.operations.iter().find(|operation| {
                operation.descriptor.native_name == interface.operation.native_name
            }) else {
                continue;
            };
            let base = format!("/api/app/{domain_code}/{}", operation.model.encode());
            let default_name = match operation.intent {
                OperationIntent::List => format!("查询{}列表", operation.model.native_name),
                OperationIntent::Read => format!("查看{}", operation.model.native_name),
                OperationIntent::Create => format!("新增{}", operation.model.native_name),
                OperationIntent::Update => format!("修改{}", operation.model.native_name),
                OperationIntent::Delete => format!("删除{}", operation.model.native_name),
                OperationIntent::Authenticate | OperationIntent::Command => String::new(),
            };
            if operation.descriptor.native_name != default_name && !default_name.is_empty() {
                interface.path = match operation.intent {
                    OperationIntent::List | OperationIntent::Create => {
                        format!("{base}/{}", operation.encode())
                    }
                    OperationIntent::Read | OperationIntent::Update | OperationIntent::Delete => {
                        format!("{base}/{{id}}/{}", operation.encode())
                    }
                    OperationIntent::Authenticate | OperationIntent::Command => base,
                };
                continue;
            }
            interface.path = match operation.intent {
                OperationIntent::List | OperationIntent::Create => base,
                OperationIntent::Read | OperationIntent::Update | OperationIntent::Delete => {
                    format!("{base}/{{id}}")
                }
                OperationIntent::Authenticate => format!("{base}/authenticate"),
                OperationIntent::Command => format!("{base}/{}", operation.encode()),
            };
        }
        for view in &mut self.views {
            view.route = format!("/{domain_code}/{}", view.encode());
        }
    }
}

/// 应用中的领域边界及其模型引用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainDefinition {
    pub descriptor: SemanticDescriptor,
    pub models: Vec<SemanticDescriptor>,
}

impl Encode for DomainDefinition {
    fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 领域操作意图，决定确定性路由和执行计划。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationIntent {
    List,
    Read,
    Create,
    Update,
    Delete,
    Authenticate,
    Command,
}

/// 不包含脚本源码的类型化领域操作。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainOperation {
    pub descriptor: SemanticDescriptor,
    pub model: SemanticDescriptor,
    pub intent: OperationIntent,
    pub steps: Vec<OperationPlanStep>,
}

impl Encode for DomainOperation {
    fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 宿主可确定性执行的操作步骤。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationPlanStep {
    ValidateInput,
    QueryRecords,
    LoadRecord,
    CreateRecord,
    UpdateRecord,
    DeleteRecord,
    InvokeCapability { capability: SemanticDescriptor },
    ReturnResult,
}

/// 编译器内部使用的 HTTP 方法。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

/// 对外领域接口；method 和 path 均由编译器推导。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceDefinition {
    pub descriptor: SemanticDescriptor,
    pub operation: SemanticDescriptor,
    pub method: HttpMethod,
    pub path: String,
}

/// 页面使用的受控布局语义。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewLayout {
    Table,
    Detail,
    Form,
}

/// 页面动作对领域操作的引用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewActionDefinition {
    pub descriptor: SemanticDescriptor,
    pub operation: SemanticDescriptor,
}

/// 与具体 UI 框架无关的页面语义。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDefinition {
    pub descriptor: SemanticDescriptor,
    pub model: SemanticDescriptor,
    pub layout: ViewLayout,
    pub fields: Vec<SemanticDescriptor>,
    pub actions: Vec<ViewActionDefinition>,
    pub route: String,
}

impl Encode for ViewDefinition {
    fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 菜单树中的页面入口。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationEntry {
    pub descriptor: SemanticDescriptor,
    pub view: SemanticDescriptor,
    pub order: i32,
    pub permissions: Vec<SemanticDescriptor>,
}

/// 应用导航定义。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationDefinition {
    pub section_label: String,
    pub descriptor: SemanticDescriptor,
    pub default_view: SemanticDescriptor,
    pub entries: Vec<NavigationEntry>,
}

/// 权限规则保持强类型，不把策略表达式交给 AI。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRule {
    Authenticated,
    OwnRecords,
    AllRecords,
}

/// 母语权限及其允许的操作。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionDefinition {
    pub descriptor: SemanticDescriptor,
    pub rule: PermissionRule,
    pub operations: Vec<SemanticDescriptor>,
}

/// 受控词表或命名空间扩展提供的领域元数据。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainMetadata {
    pub namespace: String,
    pub name: String,
    pub value: String,
}

/// 无数据业务枚举定义。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumDefinition {
    pub descriptor: SemanticDescriptor,
    pub values: Vec<EnumValueDefinition>,
}

impl EnumDefinition {
    pub fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 枚举值定义，母语标签与代码身份保持分离。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumValueDefinition {
    pub descriptor: SemanticDescriptor,
}

impl Encode for EnumValueDefinition {
    fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 可生成的领域函数定义。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDefinition {
    pub descriptor: SemanticDescriptor,
    pub input_model: SemanticDescriptor,
    pub output_model: SemanticDescriptor,
    pub steps: Vec<LogicStep>,
}

impl FunctionDefinition {
    pub fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 强类型逻辑步骤，拒绝用户可编辑的操作字符串。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogicStep {
    ReadSourceMap,
    DecodeBindings,
    ValidateFields,
    ReturnAcceptedValue,
}

/// 由母语数据源语义推导的能力需求。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityRequirement {
    pub descriptor: SemanticDescriptor,
    pub source_phrase: String,
}

impl Encode for CapabilityRequirement {
    fn encode(&self) -> &str {
        self.descriptor.encode()
    }
}

/// 领域字段和原始 Map 键之间的类型化绑定。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldBinding {
    pub owner: SemanticDescriptor,
    pub field: SemanticDescriptor,
    pub source: SemanticDescriptor,
    pub transform: ValueTransform,
}

impl Encode for FieldBinding {
    fn encode(&self) -> &str {
        self.field.encode()
    }
}

/// 原始值到领域字段的纯变换。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValueTransform {
    Identity,
    Divide { divisor: f64 },
}

/// AI 或规则引擎作出的可审查推导。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceDecision {
    pub subject: String,
    pub decision: String,
    pub reused: bool,
}

/// 编译器采用的安全默认项。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedDefault {
    pub subject: String,
    pub value: String,
    pub reason: String,
}
