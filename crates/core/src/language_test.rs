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
