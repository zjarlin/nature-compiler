# nature-compiler

`nature-compiler` 把产品经理的中文需求编译为可审查、可重复生成、可通过门禁验证的 Rust
代码。AI 只负责把母语推导成强类型 `Blueprint`，编译器负责命名、引用、能力解析、策略校验和
源码布局。产品输入不填写 `code`、`kind`、字典编码、Provider 标识、Rust 类型、表名或文件路径。

首版只包含一个协议无关的 `fixture.map` 能力：宿主传入
`BTreeMap<String, serde_json::Value>`，生成代码完成字段读取、数值变换和领域校验。数据库、文件写入、
Cargo 命令和发布动作都由宿主控制，不进入本库。

## 最小用法

```rust,no_run
use std::sync::Arc;

use nature_compiler::{
    CapabilityCatalog, CompileRequest, Compiler, MotherTongueInferenceEngine,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let compiler = Compiler::new(
        Arc::new(MotherTongueInferenceEngine),
        CapabilityCatalog::with_fixture_map(),
    );
    let result = compiler
        .compile(CompileRequest {
            source_text: "建模：用户\n1. 姓名：文本\n数据获取：\n1. 模拟采集提供姓名原值\n2. 姓名等于姓名原值".to_string(),
            previous_blueprint: None,
        })
        .await?;

    assert!(result.artifacts.is_some());
    Ok(())
}
```

真实 AI 适配器实现 `InferenceEngine`，只能返回 `Blueprint` 中定义的强类型推导树，不能返回 Rust、
SQL、Rhai、依赖或目标文件路径。能力实现 `CapabilityProvider`，由 AIO 使用 Rudi 收集后构建
`CapabilityCatalog`；新增能力不修改编译器分支。

## 生成布局

生成物固定为结构型布局：

- `src/structs.rs`
- `src/enums.rs`
- `src/functions.rs`
- `src/validators.rs`
- `src/bindings.rs`
- `src/descriptors.rs`
- `tests/generated.rs`
- `blueprint.json`

License: `MIT OR Apache-2.0`。
