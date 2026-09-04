#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Java,
    Kotlin,
    Properties,
    Yaml,
    Xml,
    Dockerfile,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "java" => Some(Language::Java),
            "kt" => Some(Language::Kotlin),
            "properties" => Some(Language::Properties),
            "yml" | "yaml" => Some(Language::Yaml),
            "xml" => Some(Language::Xml),
            "dockerfile" => Some(Language::Dockerfile),
            _ => None,
        }
    }

    /// How this language is named in the UI — the status bar's own
    /// language indicator. Deliberately not `Debug`: these are shown to
    /// users, so a future rename of a variant must not silently change
    /// what the editor says a file is.
    pub fn display_name(self) -> &'static str {
        match self {
            Language::Java => "Java",
            Language::Kotlin => "Kotlin",
            Language::Properties => "Properties",
            Language::Yaml => "YAML",
            Language::Xml => "XML",
            Language::Dockerfile => "Dockerfile",
        }
    }

    /// Recognizes a `Dockerfile` by its bare file name, for files with no
    /// extension at all to key off (`from_extension` alone can't: a plain
    /// `Dockerfile` has nothing after a `.` to look at). Matches the exact
    /// name `Dockerfile`, a variant conventionally suffixed with a stage
    /// name (`Dockerfile.dev`, `Dockerfile.prod`), or one prefixed instead
    /// (`dev.Dockerfile`) — the two orderings both appear across real
    /// projects, so both are recognized rather than picking one
    /// convention. Case-insensitive on the `Dockerfile` part itself
    /// (`dockerfile`, no extension, is also common on case-sensitive
    /// filesystems) but not on whatever stage suffix/prefix surrounds it,
    /// since that part is user-chosen free text, not a fixed keyword.
    pub fn from_filename(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        (lower == "dockerfile" || lower.starts_with("dockerfile.") || lower.ends_with(".dockerfile"))
            .then_some(Language::Dockerfile)
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
        assert_eq!(Language::from_extension("dockerfile"), Some(Language::Dockerfile));
        assert_eq!(Language::from_extension("txt"), None);
    }

    #[test]
    fn from_filename_recognizes_a_bare_dockerfile() {
        assert_eq!(Language::from_filename("Dockerfile"), Some(Language::Dockerfile));
        assert_eq!(Language::from_filename("dockerfile"), Some(Language::Dockerfile));
    }

    #[test]
    fn from_filename_recognizes_a_suffixed_or_prefixed_variant() {
        assert_eq!(Language::from_filename("Dockerfile.dev"), Some(Language::Dockerfile));
        assert_eq!(Language::from_filename("Dockerfile.prod"), Some(Language::Dockerfile));
        assert_eq!(Language::from_filename("dev.Dockerfile"), Some(Language::Dockerfile));
    }

    #[test]
    fn from_filename_rejects_an_unrelated_name() {
        assert_eq!(Language::from_filename("README"), None);
        assert_eq!(Language::from_filename("docker-compose.yml"), None);
    }
}
