#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Java,
    Kotlin,
    Properties,
    Yaml,
    Xml,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "java" => Some(Language::Java),
            "kt" => Some(Language::Kotlin),
            "properties" => Some(Language::Properties),
            "yml" | "yaml" => Some(Language::Yaml),
            "xml" => Some(Language::Xml),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_every_supported_extension() {
        assert_eq!(Language::from_extension("java"), Some(Language::Java));
        assert_eq!(Language::from_extension("kt"), Some(Language::Kotlin));
        assert_eq!(Language::from_extension("properties"), Some(Language::Properties));
        assert_eq!(Language::from_extension("yml"), Some(Language::Yaml));
        assert_eq!(Language::from_extension("yaml"), Some(Language::Yaml));
        assert_eq!(Language::from_extension("xml"), Some(Language::Xml));
        assert_eq!(Language::from_extension("txt"), None);
    }
}
