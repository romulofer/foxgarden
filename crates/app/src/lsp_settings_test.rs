use super::*;
use std::path::PathBuf;

fn installed(server: &str, version: &str, binary: &str) -> Installed {
    Installed {
        server: server.to_string(),
        version: version.to_string(),
        binary: PathBuf::from(binary),
    }
}

#[test]
fn apply_installed_writes_only_the_matching_servers_fields() {
    let mut settings = LspSettings::default().with_binary("kotlin-language-server", "/existing/kotlin");
    settings.apply_installed(&installed("jdtls", "1.60.0", "/cache/jdtls-1.60.0/bin/jdtls"));
    assert_eq!(settings.binary_for("jdtls"), "/cache/jdtls-1.60.0/bin/jdtls");
    assert_eq!(settings.servers["jdtls"].installed_version, "1.60.0");
    assert_eq!(settings.binary_for("kotlin-language-server"), "/existing/kotlin");
    assert_eq!(settings.servers["kotlin-language-server"].installed_version, "");
}

/// Reinstalling replaces a hand-typed path — see `apply_installed`'s
/// own doc comment on why that's the intended behavior rather than a
/// clobbering bug.
#[test]
fn apply_installed_overwrites_an_existing_path() {
    let mut settings = LspSettings::default().with_binary("jdtls", "/usr/local/bin/jdtls");
    settings.servers.get_mut("jdtls").unwrap().installed_version = "1.59.0".to_string();
    settings.apply_installed(&installed("jdtls", "1.60.0", "/cache/jdtls-1.60.0/bin/jdtls"));
    assert_eq!(settings.binary_for("jdtls"), "/cache/jdtls-1.60.0/bin/jdtls");
    assert_eq!(settings.servers["jdtls"].installed_version, "1.60.0");
}

#[test]
fn fields_for_addresses_each_server_separately() {
    let mut settings = LspSettings::default();
    *settings.fields_for("kotlin-language-server").0 = "/kotlin".to_string();
    assert_eq!(settings.binary_for("kotlin-language-server"), "/kotlin");
    assert_eq!(settings.binary_for("jdtls"), "");
}

#[test]
fn runtime_home_is_per_server_and_survives_an_install() {
    let mut settings = LspSettings::default();
    *settings.runtime_home_mut("jdtls") = "/jvm/21".to_string();
    assert_eq!(settings.runtime_home_for("jdtls"), "/jvm/21");
    assert_eq!(settings.runtime_home_for("kotlin-language-server"), "");

    settings.apply_installed(&installed("jdtls", "1.60.0", "/cache/jdtls/bin/jdtls"));
    assert_eq!(settings.runtime_home_for("jdtls"), "/jvm/21");
}
