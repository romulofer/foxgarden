#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Java,
    Kotlin,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "java" => Some(Language::Java),
            "kt" => Some(Language::Kotlin),
            _ => None,
        }
    }
}
