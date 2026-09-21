use super::*;

/// A real excerpt captured this session from
/// `spring-boot-autoconfigure-4.0.6.jar`'s own bundled
/// `META-INF/spring-configuration-metadata.json` — a boolean default, a
/// string default, and a property with no default at all, exactly as
/// found in the real file, not invented for the test.
const REAL_METADATA_EXCERPT: &str = r#"{
        "groups": [
            {"name": "spring.info", "type": "org.springframework.boot.autoconfigure.info.ProjectInfoProperties"}
        ],
        "properties": [
            {"name": "spring.aop.auto", "type": "java.lang.Boolean", "description": "Add @EnableAspectJAutoProxy.", "defaultValue": true},
            {"name": "spring.application.admin.jmx-name", "type": "java.lang.String", "description": "JMX name of the application admin MBean.", "defaultValue": "org.springframework.boot:type=Admin,name=SpringApplication"},
            {"name": "spring.autoconfigure.exclude", "type": "java.util.List<java.lang.Class>", "description": "Auto-configuration classes to exclude."}
        ],
        "hints": [
            {"name": "server.servlet.jsp.class-name", "providers": [{"name": "class-reference", "parameters": {"target": "jakarta.servlet.http.HttpServlet"}}]}
        ]
    }"#;

#[test]
fn parses_real_metadata_ignoring_groups_and_hints() {
    let properties = parse_metadata_json(REAL_METADATA_EXCERPT).unwrap();
    assert_eq!(properties.len(), 3);

    assert_eq!(
        properties[0],
        SpringConfigProperty {
            name: "spring.aop.auto".to_string(),
            type_name: Some("java.lang.Boolean".to_string()),
            description: Some("Add @EnableAspectJAutoProxy.".to_string()),
            default_value: Some("true".to_string()),
        }
    );
    assert_eq!(
        properties[1].default_value.as_deref(),
        Some("org.springframework.boot:type=Admin,name=SpringApplication")
    );
    assert_eq!(
        properties[2].default_value, None,
        "a property with no defaultValue at all"
    );
}

#[test]
fn a_property_with_no_description_or_type_still_parses() {
    let json = r#"{"properties": [{"name": "some.bare.property"}]}"#;
    let properties = parse_metadata_json(json).unwrap();
    assert_eq!(
        properties,
        vec![SpringConfigProperty {
            name: "some.bare.property".to_string(),
            type_name: None,
            description: None,
            default_value: None,
        }]
    );
}

#[test]
fn a_document_with_no_properties_key_at_all_yields_an_empty_list() {
    let json = r#"{"groups": []}"#;
    assert!(parse_metadata_json(json).unwrap().is_empty());
}

#[test]
fn malformed_json_is_an_error() {
    assert!(parse_metadata_json("not json").is_err());
}

#[test]
fn scanning_a_nonexistent_jar_is_an_error_not_a_silent_empty_result() {
    // Distinguishes `scan_jar_for_metadata`'s own two failure shapes: a
    // jar that exists but genuinely has no metadata entry (Ok(vec![]),
    // the ordinary case) versus one that can't even be opened at all
    // (Err) — `scan_classpath_for_metadata` is what silently drops the
    // latter across a whole batch; this single-jar function itself must
    // not conflate the two.
    assert!(scan_jar_for_metadata(Path::new("/nonexistent/path/to/some.jar")).is_err());
}

#[test]
fn scan_classpath_for_metadata_silently_skips_an_unopenable_jar() {
    let classpath = vec![std::path::PathBuf::from("/nonexistent/path/to/some.jar")];
    assert!(scan_classpath_for_metadata(&classpath).is_empty());
}
