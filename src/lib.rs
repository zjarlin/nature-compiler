#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod artifact;
pub mod blueprint;
pub mod capability;
pub mod catalog;
pub mod compiler;
pub mod descriptor;
pub mod diagnostic;
pub mod encode;
pub mod inference;
pub mod observability;
pub mod policy;
pub mod rust_backend;

pub use artifact::{ArtifactFile, ArtifactSet};
pub use blueprint::*;
pub use capability::{CapabilityCatalog, CapabilityProvider, FixtureMapProvider};
pub use catalog::{CompilerCatalog, SemanticCapabilityDefinition};
pub use compiler::{CompileRequest, CompileResult, Compiler};
pub use descriptor::{DescriptorEncoder, SemanticDescriptor, normalize_inferred_stem};
pub use diagnostic::{BreakingChange, Diagnostic, DiagnosticLevel, ImpactArea};
pub use encode::Encode;
pub use inference::{InferenceEngine, InferenceResult, MotherTongueInferenceEngine};
pub use observability::{
    CompileStage, CompileStageObservation, CompileStageStatus, CompileTrace, InferenceMetrics,
    InferenceMode,
};
pub use rust_backend::RustBackend;
