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
    Integer,
    Decimal,
    Boolean,
    Timestamp,
    Json,
}

impl FieldType {
    pub fn rust_type(&self) -> &'static str {
        match self {
            Self::String => "String",
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
    NumberRange { minimum: f64, maximum: f64 },
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
