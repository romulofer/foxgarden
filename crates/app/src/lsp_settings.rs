use crate::lsp_manager::{Installed, Server};

/// Settings > Language Servers… (`panels::lsp_servers`) — off by default:
/// spawning an external JVM process per open Java/Kotlin project is real
/// enough cost that it stays explicit opt-in, the same stance
/// `AutoSaveSettings` already takes. `lsp_state::LspState` checks `enabled`
/// before every spawn, so no external process launches until the user turns
/// this on.
///
/// The `*_installed_version` fields record what `lsp_manager` last
/// installed into its own cache directory for each server — `""` means "not
/// installed through FoxGarden", which is also the right value for someone
/// who pointed a binary field at a server they installed themselves. Same
/// shape (and same reason) as `static_analysis::ExternalToolPaths`' own
/// installed-version fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LspSettings {
    pub enabled: bool,
    pub jdtls_binary: String,
    pub jdtls_installed_version: String,
    /// Explicit `JAVA_HOME` jdt.ls itself runs under, overriding the
    /// ambient `JAVA_HOME`/`PATH` `lsp_manager::resolve_jdtls_java` would
    /// otherwise fall back to — for the common case where the system
    /// default `java` isn't Java 21 (jdt.ls' own stated runtime minimum),
    /// e.g. an sdkman/asdf-managed JDK 21 that isn't the active one. Empty
    /// means "auto-detect", same as leaving it unset.
    pub jdtls_java_home: String,
    pub kotlin_language_server_binary: String,
    pub kotlin_language_server_installed_version: String,
}

impl LspSettings {
    /// Writes a completed install's launcher path into the matching binary
    /// field, overwriting whatever was there. An explicit Install/Update
    /// click is exactly the case where overwriting a hand-typed path is
    /// right: the user just asked this app to go fetch a copy and use it.
    pub fn apply_installed(&mut self, installed: &Installed) {
        match installed.server {
            Server::Jdtls => {
                self.jdtls_binary = installed.binary.display().to_string();
                self.jdtls_installed_version = installed.version.clone();
            }
            Server::KotlinLanguageServer => {
                self.kotlin_language_server_binary = installed.binary.display().to_string();
                self.kotlin_language_server_installed_version = installed.version.clone();
            }
        }
    }

    /// This server's configured binary path and last-installed version, as
    /// the settings dialog's own per-server section needs them — mutable,
    /// since that section edits the path in place.
    pub fn fields_for(&mut self, server: Server) -> (&mut String, &mut String) {
        match server {
            Server::Jdtls => (&mut self.jdtls_binary, &mut self.jdtls_installed_version),
            Server::KotlinLanguageServer => (
                &mut self.kotlin_language_server_binary,
                &mut self.kotlin_language_server_installed_version,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn apply_installed_writes_only_the_matching_servers_fields() {
        let mut settings = LspSettings {
            kotlin_language_server_binary: "/existing/kotlin".to_string(),
            ..Default::default()
        };
        settings.apply_installed(&Installed {
            server: Server::Jdtls,
            version: "1.60.0".to_string(),
            binary: PathBuf::from("/cache/jdtls-1.60.0/bin/jdtls"),
        });
        assert_eq!(settings.jdtls_binary, "/cache/jdtls-1.60.0/bin/jdtls");
        assert_eq!(settings.jdtls_installed_version, "1.60.0");
        assert_eq!(settings.kotlin_language_server_binary, "/existing/kotlin");
        assert_eq!(settings.kotlin_language_server_installed_version, "");
    }

    /// Reinstalling replaces a hand-typed path — see `apply_installed`'s
    /// own doc comment on why that's the intended behavior rather than a
    /// clobbering bug.
    #[test]
    fn apply_installed_overwrites_an_existing_path() {
        let mut settings = LspSettings {
            jdtls_binary: "/usr/local/bin/jdtls".to_string(),
            jdtls_installed_version: "1.59.0".to_string(),
            ..Default::default()
        };
        settings.apply_installed(&Installed {
            server: Server::Jdtls,
            version: "1.60.0".to_string(),
            binary: PathBuf::from("/cache/jdtls-1.60.0/bin/jdtls"),
        });
        assert_eq!(settings.jdtls_binary, "/cache/jdtls-1.60.0/bin/jdtls");
        assert_eq!(settings.jdtls_installed_version, "1.60.0");
    }

    #[test]
    fn fields_for_addresses_each_server_separately() {
        let mut settings = LspSettings::default();
        *settings.fields_for(Server::KotlinLanguageServer).0 = "/kotlin".to_string();
        assert_eq!(settings.kotlin_language_server_binary, "/kotlin");
        assert_eq!(settings.jdtls_binary, "");
    }
}
