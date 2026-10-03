
use super::*;

fn prop(name: &str) -> SpringConfigProperty {
    SpringConfigProperty {
        name: name.to_string(),
        type_name: None,
        description: None,
        default_value: None,
    }
}

fn prop_typed(name: &str, type_name: &str, default_value: &str) -> SpringConfigProperty {
    SpringConfigProperty {
        name: name.to_string(),
        type_name: Some(type_name.to_string()),
        description: None,
        default_value: Some(default_value.to_string()),
    }
}

#[test]
fn properties_completion_candidates_offers_every_full_dotted_name() {
    let properties = vec![prop("server.port"), prop("spring.application.name")];
    let candidates = properties_completion_candidates(&properties);
    let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, vec!["server.port", "spring.application.name"]);
    assert!(candidates.iter().all(|c| c.kind == CompletionKind::Property));
}

#[test]
fn properties_completion_candidates_detail_combines_type_and_default() {
    let properties = vec![prop_typed("server.port", "java.lang.Integer", "8080")];
    let candidates = properties_completion_candidates(&properties);
    assert_eq!(candidates[0].detail.as_deref(), Some("java.lang.Integer = 8080"));
}

#[test]
fn yaml_completion_candidates_at_top_level_offers_first_segments_only() {
    let properties = vec![
        prop("server.port"),
        prop("server.servlet.jsp.class-name"),
        prop("spring.application.name"),
    ];
    let candidates = yaml_completion_candidates(&properties, "");
    let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["server", "spring"],
        "deduped to one candidate per distinct top-level segment"
    );
}

#[test]
fn yaml_completion_candidates_one_level_down_offers_the_next_segment() {
    let properties = vec![
        prop("server.port"),
        prop("server.servlet.jsp.class-name"),
        prop("spring.application.name"),
    ];
    let candidates = yaml_completion_candidates(&properties, "server.");
    let labels: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, vec!["port", "servlet"]);
}

#[test]
fn yaml_completion_candidates_a_leaf_property_gets_its_own_detail_a_prefix_only_segment_does_not() {
    let properties = vec![
        prop_typed("server.port", "java.lang.Integer", "8080"),
        prop("server.servlet.jsp.class-name"),
    ];
    let candidates = yaml_completion_candidates(&properties, "server.");
    let port = candidates.iter().find(|c| c.label == "port").unwrap();
    let servlet = candidates.iter().find(|c| c.label == "servlet").unwrap();
    assert_eq!(port.detail.as_deref(), Some("java.lang.Integer = 8080"));
    assert_eq!(
        servlet.detail, None,
        "servlet is only ever a prefix at this level, not a leaf property itself"
    );
}

#[test]
fn yaml_completion_candidates_returns_nothing_below_a_leaf_that_goes_no_deeper() {
    let properties = vec![prop("server.port")];
    assert!(yaml_completion_candidates(&properties, "server.port.").is_empty());
}

#[test]
fn yaml_ancestor_path_at_top_level_is_empty() {
    let source = "server:\n  port: 8080\n";
    assert_eq!(yaml_ancestor_path(source, 0, 0), "");
}

#[test]
fn yaml_ancestor_path_one_level_nested() {
    let source = "server:\n  po\n";
    // Line 1 ("  po") is indented 2; "server:" (indent 0) is its ancestor.
    assert_eq!(yaml_ancestor_path(source, 1, 2), "server.");
}

#[test]
fn yaml_ancestor_path_several_levels_nested() {
    let source = "\
server:
  servlet:
    context-path: /api
spring:
  datasource:
    hikari:
      po
";
    // The last line ("      po", indent 6) nests under hikari(4) <-
    // datasource(2) <- spring(0) — NOT server/servlet, which are a
    // separate, already-closed sibling branch above spring's own line.
    let lines: Vec<&str> = source.lines().collect();
    let current_line = lines.len() - 1;
    assert_eq!(yaml_ancestor_path(source, current_line, 6), "spring.datasource.hikari.");
}

#[test]
fn yaml_ancestor_path_skips_blank_lines_and_comments_without_breaking_the_chain() {
    let source = "\
server:
  # a comment
  servlet:

    po
";
    let lines: Vec<&str> = source.lines().collect();
    let current_line = lines.len() - 1;
    assert_eq!(yaml_ancestor_path(source, current_line, 4), "server.servlet.");
}

#[test]
fn yaml_ancestor_path_ignores_a_list_item_line() {
    let source = "\
server:
  names:
    - foo
  po
";
    // "- foo" (indent 4) is deeper than "po"'s own indent (2), so it's
    // never even considered as this line's ancestor regardless of its
    // list-item shape; "names:" (indent 2) is a sibling, not an
    // ancestor, since it's not strictly shallower than "po"'s indent 2.
    let lines: Vec<&str> = source.lines().collect();
    let current_line = lines.len() - 1;
    assert_eq!(yaml_ancestor_path(source, current_line, 2), "server.");
}
