use super::*;
use crate::IncrementalParser;
use fg_core::Language;

fn parsed(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Java).expect("a bundled grammar must load");
    parser.parse(source).clone()
}

fn endpoints(source: &str) -> Vec<EndpointInfo> {
    let tree = parsed(source);
    java_endpoints_in_file(&tree, source)
}

fn parsed_kotlin(source: &str) -> Tree {
    let mut parser = IncrementalParser::new(Language::Kotlin).expect("a bundled grammar must load");
    parser.parse(source).clone()
}

fn kotlin_endpoints(source: &str) -> Vec<EndpointInfo> {
    let tree = parsed_kotlin(source);
    kotlin_endpoints_in_file(&tree, source)
}

#[test]
fn positional_string_path() {
    let source = "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].http_method, "GET");
    assert_eq!(eps[0].path, "/x");
    assert_eq!(eps[0].controller_name, "Foo");
    assert_eq!(eps[0].handler_name, "run");
}

#[test]
fn value_equals_path() {
    let source = "class Foo {\n    @GetMapping(value = \"/x\")\n    public void run() {}\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn path_equals_path() {
    let source = "class Foo {\n    @GetMapping(path = \"/x\")\n    public void run() {}\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn method_equals_combined_with_class_level_base_path() {
    let source = "@RequestMapping(\"/api\")\nclass Foo {\n    @RequestMapping(method = RequestMethod.DELETE)\n    public void run() {}\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps[0].http_method, "DELETE");
    assert_eq!(eps[0].path, "/api");
}

#[test]
fn bare_marker_annotation_with_no_args() {
    let source = "class Foo {\n    @PostMapping\n    public void run() {}\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps[0].http_method, "POST");
    assert_eq!(eps[0].path, "/");
}

#[test]
fn no_class_level_base_path() {
    let source = "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn nested_class() {
    let source =
        "class Outer {\n    class Inner {\n        @GetMapping(\"/inner\")\n        public void run() {}\n    }\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].controller_name, "Inner");
    assert_eq!(eps[0].path, "/inner");
}

#[test]
fn multiple_unrelated_annotations_only_recognized_one_counts() {
    let source =
        "class Foo {\n    @Override\n    @GetMapping(\"/x\")\n    @Transactional\n    public void run() {}\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].http_method, "GET");
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn no_recognized_annotation_at_all() {
    let source = "class Foo {\n    @Override\n    public void run() {}\n}\n";
    assert_eq!(endpoints(source), vec![]);
}

#[test]
fn handler_byte_points_at_the_method_name() {
    let source = "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n";
    let eps = endpoints(source);
    let byte = eps[0].handler_byte;
    assert_eq!(&source[byte..byte + 3], "run");
}

#[test]
fn class_with_request_mapping_but_no_recognized_method_annotations_contributes_nothing() {
    let source = "@RequestMapping(\"/api\")\nclass Foo {\n    public void run() {}\n}\n";
    assert_eq!(endpoints(source), vec![]);
}

#[test]
fn multiple_endpoints_in_source_order() {
    let source = "@RequestMapping(\"/api\")\nclass Foo {\n    @GetMapping(\"/a\")\n    public void a() {}\n\n    @PostMapping(\"/b\")\n    public void b() {}\n}\n";
    let eps = endpoints(source);
    assert_eq!(eps.len(), 2);
    assert_eq!(eps[0].handler_name, "a");
    assert_eq!(eps[0].path, "/api/a");
    assert_eq!(eps[1].handler_name, "b");
    assert_eq!(eps[1].path, "/api/b");
}

#[test]
fn kotlin_positional_string_path() {
    let source = "class Foo {\n    @GetMapping(\"/x\")\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].http_method, "GET");
    assert_eq!(eps[0].path, "/x");
    assert_eq!(eps[0].controller_name, "Foo");
    assert_eq!(eps[0].handler_name, "run");
}

#[test]
fn kotlin_value_equals_path() {
    let source = "class Foo {\n    @GetMapping(value = \"/x\")\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn kotlin_path_equals_path() {
    let source = "class Foo {\n    @GetMapping(path = \"/x\")\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn kotlin_method_equals_combined_with_class_level_base_path() {
    let source = "@RequestMapping(\"/api\")\nclass Foo {\n    @RequestMapping(method = [RequestMethod.DELETE])\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps[0].http_method, "DELETE");
    assert_eq!(eps[0].path, "/api");
}

#[test]
fn kotlin_array_literal_unwrapping_for_path_too() {
    let source = "class Foo {\n    @GetMapping(path = [\"/x\"])\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn kotlin_bare_marker_annotation_with_no_args() {
    let source = "class Foo {\n    @PostMapping\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps[0].http_method, "POST");
    assert_eq!(eps[0].path, "/");
}

#[test]
fn kotlin_no_class_level_base_path() {
    let source = "class Foo {\n    @GetMapping(\"/x\")\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn kotlin_nested_class() {
    let source = "class Outer {\n    class Inner {\n        @GetMapping(\"/inner\")\n        fun run() {}\n    }\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].controller_name, "Inner");
    assert_eq!(eps[0].path, "/inner");
}

#[test]
fn kotlin_multiple_unrelated_annotations_only_recognized_one_counts() {
    let source =
        "class Foo {\n    @Suppress(\"unused\")\n    @GetMapping(\"/x\")\n    @JvmStatic\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps.len(), 1);
    assert_eq!(eps[0].http_method, "GET");
    assert_eq!(eps[0].path, "/x");
}

#[test]
fn kotlin_no_recognized_annotation_at_all() {
    let source = "class Foo {\n    @Suppress(\"unused\")\n    fun run() {}\n}\n";
    assert_eq!(kotlin_endpoints(source), vec![]);
}

#[test]
fn kotlin_handler_byte_points_at_the_function_name() {
    let source = "class Foo {\n    @GetMapping(\"/x\")\n    fun run() {}\n}\n";
    let eps = kotlin_endpoints(source);
    let byte = eps[0].handler_byte;
    assert_eq!(&source[byte..byte + 3], "run");
}

#[test]
fn kotlin_class_with_request_mapping_but_no_recognized_function_annotations_contributes_nothing() {
    let source = "@RequestMapping(\"/api\")\nclass Foo {\n    fun run() {}\n}\n";
    assert_eq!(kotlin_endpoints(source), vec![]);
}

#[test]
fn kotlin_multiple_endpoints_in_source_order() {
    let source = "@RequestMapping(\"/api\")\nclass Foo {\n    @GetMapping(\"/a\")\n    fun a() {}\n\n    @PostMapping(\"/b\")\n    fun b() {}\n}\n";
    let eps = kotlin_endpoints(source);
    assert_eq!(eps.len(), 2);
    assert_eq!(eps[0].handler_name, "a");
    assert_eq!(eps[0].path, "/api/a");
    assert_eq!(eps[1].handler_name, "b");
    assert_eq!(eps[1].path, "/api/b");
}

#[test]
fn dispatcher_routes_java_and_kotlin_to_their_own_producers() {
    let java_tree = parsed("class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n");
    let java_source = "class Foo {\n    @GetMapping(\"/x\")\n    public void run() {}\n}\n";
    assert_eq!(
        endpoints_in_file(Language::Java, &java_tree, java_source),
        java_endpoints_in_file(&java_tree, java_source)
    );

    let kotlin_source = "class Foo {\n    @GetMapping(\"/x\")\n    fun run() {}\n}\n";
    let kotlin_tree = parsed_kotlin(kotlin_source);
    assert_eq!(
        endpoints_in_file(Language::Kotlin, &kotlin_tree, kotlin_source),
        kotlin_endpoints_in_file(&kotlin_tree, kotlin_source)
    );
}

#[test]
fn dispatcher_returns_empty_for_an_unsupported_language() {
    let tree = parsed("class Foo {}\n");
    assert_eq!(endpoints_in_file(Language::Yaml, &tree, "class Foo {}\n"), vec![]);
}
