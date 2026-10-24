use crate::{Capability, Contributions, Extension, ExtensionManifest};

/// What one extension brings, in the words the UI shows for it: the
/// extension manager lists this without needing the extension to be
/// registered, so a disabled one can still be read about (and switched back
/// on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionInfo {
    pub manifest: ExtensionManifest,
    pub languages: Vec<String>,
    pub language_servers: Vec<String>,
    pub build_tools: Vec<String>,
    pub analyzers: Vec<String>,
    pub capabilities: Vec<Capability>,
}

impl ExtensionInfo {
    pub fn of(extension: &dyn Extension) -> Self {
        let manifest = extension.manifest();
        let Contributions {
            languages,
            language_servers,
            build_tools,
            analyzers,
            ..
        } = extension.contributions();
        Self {
            capabilities: extension.capabilities(),
            manifest,
            languages: languages.into_iter().map(|l| l.display_name).collect(),
            language_servers: language_servers.into_iter().map(|s| s.display_name).collect(),
            build_tools: build_tools.into_iter().map(|t| t.display_name).collect(),
            analyzers: analyzers.into_iter().map(|a| a.display_name).collect(),
        }
    }
}
