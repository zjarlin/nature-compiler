use std::sync::Arc;

use anyhow::{Result, bail};
use nature_compiler::{CompileRequest, Compiler, CompilerCatalog, MotherTongueInferenceEngine};

const SOURCE: &str = include_str!("../tests/fixtures/environment.txt");

#[tokio::main]
async fn main() -> Result<()> {
    let compiler = Compiler::new(
        Arc::new(MotherTongueInferenceEngine),
        CompilerCatalog::with_fixture_map(),
    );
    let result = compiler
        .compile(CompileRequest {
            source_text: SOURCE.to_string(),
            previous_blueprint: None,
        })
        .await?;
    let Some(artifacts) = result.artifacts else {
        for diagnostic in result.diagnostics {
            eprintln!("{}：{}", diagnostic.subject, diagnostic.message);
        }
        bail!("环境采集编译失败");
    };
    println!("{}", artifacts.hash);
    for file in artifacts.files {
        println!("{} {}", file.relative_path, file.source.len());
    }
    Ok(())
}
