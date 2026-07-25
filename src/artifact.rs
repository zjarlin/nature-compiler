use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 单个确定性生成文件。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFile {
    pub relative_path: String,
    pub source: String,
}

/// 完整生成物及其内容摘要。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSet {
    pub files: Vec<ArtifactFile>,
    pub hash: String,
}

impl ArtifactSet {
    /// 规范化文件顺序并计算逐字节稳定摘要。
    pub fn new(mut files: Vec<ArtifactFile>) -> Self {
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut digest = Sha256::new();
        for file in &files {
            digest.update(file.relative_path.as_bytes());
            digest.update([0]);
            digest.update(file.source.as_bytes());
            digest.update([0]);
        }
        let hash = format!("{:x}", digest.finalize());
        Self { files, hash }
    }

    /// 按相对路径读取生成文件。
    pub fn file(&self, relative_path: &str) -> Option<&ArtifactFile> {
        self.files
            .iter()
            .find(|file| file.relative_path == relative_path)
    }
}
