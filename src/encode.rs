/// 把母语语义元素映射为确定性的代码身份。
pub trait Encode {
    fn encode(&self) -> &str;
}
