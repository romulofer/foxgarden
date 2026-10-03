
use super::*;

#[test]
fn word_before_cursor_finds_the_trailing_identifier() {
    let text = "System.out.println(sout";
    let expected_start = text.find("sout").unwrap();
    assert_eq!(
        word_before_cursor(text, text.chars().count()),
        expected_start..text.chars().count()
    );
}

#[test]
fn word_before_cursor_is_empty_right_after_whitespace() {
    assert_eq!(word_before_cursor("foo ", 4), 4..4);
}

#[test]
fn word_before_cursor_at_start_of_buffer_is_empty() {
    assert_eq!(word_before_cursor("", 0), 0..0);
}

#[test]
fn identifiers_in_empty_buffer_is_empty() {
    assert_eq!(identifiers_in(""), Vec::<String>::new());
}

#[test]
fn identifiers_in_single_word() {
    assert_eq!(identifiers_in("foo"), vec!["foo"]);
}

#[test]
fn identifiers_in_splits_on_punctuation() {
    assert_eq!(identifiers_in("foo.bar(baz, 1);"), vec!["foo", "bar", "baz", "1"]);
}

#[test]
fn identifiers_in_deduplicates_in_first_seen_order() {
    assert_eq!(identifiers_in("foo bar foo baz bar"), vec!["foo", "bar", "baz"]);
}

#[test]
fn identifiers_in_keeps_underscores_as_part_of_a_word() {
    assert_eq!(identifiers_in("my_var another"), vec!["my_var", "another"]);
}

#[test]
fn find_template_matches_by_exact_trigger() {
    assert_eq!(
        find_template(JAVA_TEMPLATES, "sout"),
        Some("System.out.println(${cursor});")
    );
    assert_eq!(find_template(JAVA_TEMPLATES, "so"), None);
    assert_eq!(find_template(JAVA_TEMPLATES, ""), None);
}

#[test]
fn expand_replaces_the_trigger_and_places_cursor_at_the_marker() {
    let before = "System.out.println(sout);";
    let word_start = before.find("sout").unwrap();
    let word_range = word_start..word_start + "sout".chars().count();

    let (text, cursor) = expand(before, word_range, "System.out.println(${cursor});");
    assert_eq!(text, "System.out.println(System.out.println(););");
    // Cursor lands right where "${cursor}" was, i.e. right after the
    // expansion's own opening paren.
    assert_eq!(&text[..cursor], "System.out.println(System.out.println(");
}

#[test]
fn expand_with_no_marker_places_cursor_after_the_whole_expansion() {
    let (text, cursor) = expand("psvm", 0..4, "public static void main() {}");
    assert_eq!(text, "public static void main() {}");
    assert_eq!(cursor, text.chars().count());
}

#[test]
fn expand_multiline_template_lands_cursor_on_the_indented_body_line() {
    let (text, cursor) = expand(
        "psvm",
        0..4,
        "public static void main(String[] args) {\n    ${cursor}\n}",
    );
    assert_eq!(text, "public static void main(String[] args) {\n    \n}");
    assert_eq!(&text[cursor..], "\n}");
}

#[test]
fn find_expansion_falls_back_to_the_built_in_table_when_custom_has_no_match() {
    assert_eq!(
        find_expansion(&[], &[JAVA_TEMPLATES], "sout"),
        Some("System.out.println(${cursor});")
    );
}

#[test]
fn find_expansion_prefers_a_custom_template_over_a_built_in_of_the_same_trigger() {
    let custom = [UserTemplate {
        trigger: "sout".to_string(),
        body: "my.own.println(${cursor});".to_string(),
    }];
    assert_eq!(
        find_expansion(&[&custom], &[JAVA_TEMPLATES], "sout"),
        Some("my.own.println(${cursor});")
    );
}

#[test]
fn find_expansion_finds_a_custom_only_trigger() {
    let custom = [UserTemplate {
        trigger: "myown".to_string(),
        body: "custom body".to_string(),
    }];
    assert_eq!(
        find_expansion(&[&custom], &[JAVA_TEMPLATES], "myown"),
        Some("custom body")
    );
    assert_eq!(find_expansion(&[&custom], &[JAVA_TEMPLATES], "nope"), None);
}

#[test]
fn find_expansion_falls_through_to_the_global_built_in_group() {
    // "pipe" lives only in `GLOBAL_TEMPLATES`, not `JAVA_TEMPLATES` —
    // still found once that group is included in the lookup, the same
    // way `widget::show` includes it regardless of a file's language.
    assert_eq!(
        find_expansion(&[], &[JAVA_TEMPLATES, GLOBAL_TEMPLATES], "pipe"),
        Some("|")
    );
}

#[test]
fn find_expansion_checks_custom_groups_in_order() {
    let language_custom = [UserTemplate {
        trigger: "pipe".to_string(),
        body: "language override".to_string(),
    }];
    let global_custom = [UserTemplate {
        trigger: "pipe".to_string(),
        body: "global override".to_string(),
    }];
    // The language-specific custom group is listed first, so it wins
    // over the global custom group even though both define "pipe".
    assert_eq!(
        find_expansion(&[&language_custom, &global_custom], &[], "pipe"),
        Some("language override")
    );
    // With no language-specific match, the global custom group is
    // still reached.
    assert_eq!(
        find_expansion(&[&[], &global_custom], &[], "pipe"),
        Some("global override")
    );
}

#[test]
fn user_templates_round_trip_through_serialization() {
    let templates = vec![
        UserTemplate {
            trigger: "sout2".to_string(),
            body: "System.out.println(${cursor});".to_string(),
        },
        UserTemplate {
            trigger: "psvm2".to_string(),
            body: "public static void main(String[] args) {\n    ${cursor}\n}".to_string(),
        },
    ];
    let serialized = serialize_user_templates(&templates);
    assert_eq!(parse_user_templates(&serialized), templates);
}

#[test]
fn a_body_containing_a_literal_backslash_round_trips() {
    let templates = vec![UserTemplate {
        trigger: "path".to_string(),
        body: "C:\\Users\\${cursor}".to_string(),
    }];
    let serialized = serialize_user_templates(&templates);
    assert_eq!(parse_user_templates(&serialized), templates);
}

#[test]
fn empty_input_parses_to_no_user_templates() {
    assert_eq!(parse_user_templates(""), vec![]);
}
