#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod artifact;
pub mod blueprint;
pub mod capability;
pub mod compiler;
pub mod descriptor;
pub mod diagnostic;
pub mod encode;
pub mod inference;
pub mod policy;
pub mod rust_backend;

pub use artifact::{ArtifactFile, ArtifactSet};
pub use blueprint::*;
pub use capability::{CapabilityCatalog, CapabilityProvider, FixtureMapProvider};
pub use compiler::{CompileRequest, CompileResult, Compiler};
pub use descriptor::SemanticDescriptor;
pub use diagnostic::{BreakingChange, Diagnostic, DiagnosticLevel, ImpactArea};
pub use encode::Encode;
pub use inference::{InferenceEngine, InferenceResult, MotherTongueInferenceEngine};
pub use rust_backend::RustBackend;
