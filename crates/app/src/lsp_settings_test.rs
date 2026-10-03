
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
