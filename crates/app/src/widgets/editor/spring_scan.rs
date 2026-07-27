//! Whole-project scan for the Spring endpoint map popup (`SPEC.md` §4) —
//! walks every `.java`/`.kt` file in the project tree, parses each
//! throwaway (same "read + throwaway-parse" sequence Override Method/
//! dot-completion's cross-project lookup already do for a single
//! resolved-by-name file, just looped over every file here instead), and
//! collects whatever `syntax::endpoints_in_file` finds. Not `codegen.rs`:
//! that file is its own cohesive "code generation" concern (getter/setter/
//! constructor/`toString`/`equals`+`hashCode`, plus the shared file-finder)
//! this scan doesn't belong in — it doesn't generate anything, and shares
//! no logic with those functions beyond the same recursive-tree-walk shape
//! `find_source_file_by_stem`/`go_to_file.rs`'s `all_files` already use
//! independently of each other.

use std::path::PathBuf;

use fg_core::{FileKind, FileNode, Language};
use syntax::{EndpointInfo, IncrementalParser};

/// Every Spring MVC endpoint found under `root`, each paired with the path
/// of the file it was declared in — §6's jump needs to know which file to
/// open, and `EndpointInfo` itself carries no per-file identity of its own.
/// A file that fails to read (permission error, race with a delete) is
/// skipped silently, same "don't fail the whole operation over one bad
/// entry" reasoning already established elsewhere in this codebase. No
/// caching: this walks + parses fresh every call, correct by construction
/// at the cost of not being free — acceptable since it only runs once per
/// popup open (`SPEC.md` §0), not on a timer or per keystroke.
pub fn scan_project_endpoints(root: &FileNode) -> Vec<(PathBuf, EndpointInfo)> {
    let mut out = Vec::new();
    collect_endpoints(root, &mut out);
    out
}

fn collect_endpoints(node: &FileNode, out: &mut Vec<(PathBuf, EndpointInfo)>) {
    match node.kind {
        FileKind::Dir => {
            for child in &node.children {
                collect_endpoints(child, out);
            }
        }
        FileKind::File => {
            let Some(language) = node
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(Language::from_extension)
            else {
                return;
            };
            if !matches!(language, Language::Java | Language::Kotlin) {
                return;
            }
            let Ok(source) = std::fs::read_to_string(&node.path) else {
                return;
            };
            let mut parser = IncrementalParser::new(language);
            let tree = parser.parse(&source).clone();
            for endpoint in syntax::endpoints_in_file(language, &tree, &source) {
                out.push((node.path.clone(), endpoint));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fg_core::Project;

    fn scan(files: &[(&str, &str)]) -> Vec<(PathBuf, EndpointInfo)> {
        let dir = tempfile::tempdir().unwrap();
        for (name, contents) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
        }
        let project = Project::open(dir.path().to_path_buf()).unwrap();
        scan_project_endpoints(&project.tree)
    }

    #[test]
    fn aggregates_endpoints_across_java_and_kotlin_files_with_the_right_paths() {
        let results = scan(&[
            (
                "UserController.java",
                "@RequestMapping(\"/api\")\nclass UserController {\n    @GetMapping(\"/{id}\")\n    public void getUser() {}\n}\n",
            ),
            (
                "OrderController.kt",
                "class OrderController {\n    @PostMapping(\"/orders\")\n    fun createOrder() {}\n}\n",
            ),
            ("NotAController.java", "class NotAController {\n    public void run() {}\n}\n"),
        ]);

        assert_eq!(results.len(), 2);

        let java_entry = results.iter().find(|(_, e)| e.handler_name == "getUser").unwrap();
        assert!(java_entry.0.ends_with("UserController.java"));
        assert_eq!(java_entry.1.path, "/api/{id}");

        let kotlin_entry = results.iter().find(|(_, e)| e.handler_name == "createOrder").unwrap();
        assert!(kotlin_entry.0.ends_with("OrderController.kt"));
        assert_eq!(kotlin_entry.1.path, "/orders");
    }

    #[test]
    fn a_non_source_file_in_the_tree_is_ignored() {
        let results = scan(&[("README.md", "# hi"), ("pom.xml", "<project/>")]);
        assert_eq!(results, vec![]);
    }

    #[test]
    fn an_empty_project_returns_empty() {
        let results = scan(&[]);
        assert_eq!(results, vec![]);
    }
}
