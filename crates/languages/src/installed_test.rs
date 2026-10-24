use super::*;
use fg_extension::Registry;

const MINIMAL: &str = r#"
id = "toml-lang"
name = "TOML"
version = "0.1.0"
schema_version = 0

[[languages]]
id = "toml"
display_name = "TOML"
file_extensions = ["TOML"]
filename_patterns = [{ stem = "Cargo", match = "with_affix" }]

[[grammars]]
language_id = "toml"
library = "toml.so"
symbol = "tree_sitter_toml"
highlights = "highlights.scm"
node_kinds = { scopes = ["table"], foldable = ["table"], import = "use" }

[[file_icons]]
language_id = "toml"
glyph = ""

[[snippets]]
language_id = "toml"
trigger = "dep"
body = "name = \"$0\""

[[keywords]]
language_id = "toml"
words = ["true", "false"]
"#;

fn install(root: &Path, dir: &str, manifest: &str) -> PathBuf {
    let dir = root.join(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    dir
}

fn with_query(dir: &Path) {
    std::fs::write(dir.join("highlights.scm"), "(comment) @comment").unwrap();
}

#[test]
fn a_manifest_becomes_the_contributions_it_declares() {
    let root = tempfile::tempdir().unwrap();
    let dir = install(root.path(), "toml-lang", MINIMAL);
    with_query(&dir);

    let extension = InstalledExtension::load(&dir).unwrap();
    assert_eq!(extension.manifest().id, "toml-lang");
    let contributions = extension.contributions();

    let language = &contributions.languages[0];
    assert_eq!(language.file_extensions, vec!["toml"], "extensions are matched lowercase");
    assert!(language.filename_patterns[0].matches("Cargo.lock"));

    let grammar = &contributions.grammars[0];
    assert_eq!(grammar.highlight_query.as_deref(), Some("(comment) @comment"));
    assert_eq!(grammar.node_kinds.import.as_deref(), Some("use"));
    let GrammarSource::SharedLibrary { path, symbol } = &grammar.source else {
        panic!("an installed grammar is always a shared library: {:?}", grammar.source);
    };
    assert_eq!(path, &dir.join("toml.so"));
    assert_eq!(symbol, "tree_sitter_toml");

    assert_eq!(contributions.file_icons[0].glyph, '\u{e6b2}');
    assert_eq!(extension.snippets("toml")[0].trigger, "dep");
    assert!(extension.snippets("yaml").is_empty());
    assert_eq!(extension.keywords("toml"), vec!["true", "false"]);
}

#[test]
fn an_unknown_table_is_refused_instead_of_ignored() {
    let root = tempfile::tempdir().unwrap();
    let manifest = format!("{MINIMAL}\n[[build_tools]]\nid = \"cargo\"\n");
    let dir = install(root.path(), "toml-lang", &manifest);
    with_query(&dir);

    let error = InstalledExtension::load(&dir).err().expect("a build tool table is not loadable yet");
    assert!(error.message.contains("build_tools"), "{error}");
}

#[test]
fn a_path_leaving_the_extension_directory_is_refused() {
    let root = tempfile::tempdir().unwrap();
    for escaping in ["../other/toml.so", "/usr/lib/toml.so"] {
        let manifest = MINIMAL.replace("toml.so", escaping);
        let dir = install(root.path(), "toml-lang", &manifest);
        with_query(&dir);
        let error = InstalledExtension::load(&dir).err().expect(escaping);
        assert!(error.message.contains("inside the extension directory"), "{escaping}: {error}");
    }
}

#[test]
fn a_missing_highlight_query_names_the_file() {
    let root = tempfile::tempdir().unwrap();
    let dir = install(root.path(), "toml-lang", MINIMAL);
    let error = InstalledExtension::load(&dir).err().expect("highlights.scm was never written");
    assert!(error.message.contains("highlights.scm"), "{error}");
}

#[test]
fn an_unknown_filename_match_kind_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let dir = install(root.path(), "toml-lang", &MINIMAL.replace("with_affix", "fuzzy"));
    with_query(&dir);
    let error = InstalledExtension::load(&dir).err().unwrap();
    assert!(error.message.contains("fuzzy"), "{error}");
}

#[test]
fn one_bad_extension_does_not_stop_the_others() {
    let root = tempfile::tempdir().unwrap();
    install(root.path(), "a-broken", "this is not toml = = =");
    let good = install(root.path(), "b-good", MINIMAL);
    with_query(&good);
    std::fs::create_dir(root.path().join("not-an-extension")).unwrap();

    let (loaded, errors) = load_all(root.path());
    assert_eq!(loaded.len(), 1);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].dir.ends_with("a-broken"));
}

#[test]
fn a_missing_extensions_directory_is_no_extensions() {
    let (loaded, errors) = load_all(Path::new("/nonexistent/foxgarden/extensions"));
    assert!(loaded.is_empty() && errors.is_empty());
}

#[test]
fn installed_extensions_register_next_to_the_built_in_ones() {
    let root = tempfile::tempdir().unwrap();
    let dir = install(root.path(), "toml-lang", MINIMAL);
    with_query(&dir);

    let mut registry = crate::builtin_registry();
    let problems = crate::register_installed(&mut registry, root.path(), &[]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(registry.language_for_extension("toml").unwrap().language.id, "toml");
    let handle = registry
        .handles()
        .into_iter()
        .find(|h| &*h.id == "toml-lang")
        .expect("registered");
    assert_eq!(handle.snippets("toml").len(), 1, "the registered extension must answer for its language");
}

#[test]
fn a_disabled_installed_extension_is_not_registered() {
    let root = tempfile::tempdir().unwrap();
    let dir = install(root.path(), "toml-lang", MINIMAL);
    with_query(&dir);

    let mut registry = Registry::new();
    let problems = crate::register_installed(&mut registry, root.path(), &["toml-lang".to_string()]);
    assert!(problems.is_empty());
    assert!(registry.language_for_extension("toml").is_none());
}

#[test]
fn a_clash_with_a_built_in_language_is_reported_and_leaves_the_registry_alone() {
    let root = tempfile::tempdir().unwrap();
    let manifest = MINIMAL.replace(r#"id = "toml"
display_name"#, r#"id = "yaml"
display_name"#);
    let dir = install(root.path(), "toml-lang", &manifest);
    with_query(&dir);

    let mut registry = crate::builtin_registry();
    let before = registry.languages().count();
    let problems = crate::register_installed(&mut registry, root.path(), &[]);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(registry.languages().count(), before);
}

#[test]
fn declared_capabilities_are_read_and_unknown_ones_refused() {
    let root = tempfile::tempdir().unwrap();
    let declared = format!("capabilities = [\"process_exec\"]\n{MINIMAL}");
    let dir = install(root.path(), "a", &declared);
    with_query(&dir);
    let extension = InstalledExtension::load(&dir).unwrap();
    assert_eq!(extension.capabilities(), vec![Capability::ProcessExec]);

    let unknown = format!("capabilities = [\"root\"]\n{MINIMAL}");
    let dir = install(root.path(), "b", &unknown);
    with_query(&dir);
    let error = InstalledExtension::load(&dir).err().expect("root is not a capability");
    assert!(error.message.contains("\"root\""), "{error}");
}

/// A data-only extension for a language of its own, with a server for it.
fn with_server(extension_id: &str, capabilities: &str) -> String {
    format!(
        r#"
id = "{extension_id}"
name = "Taplo"
version = "0.1.0"
schema_version = 0
capabilities = [{capabilities}]

[[languages]]
id = "{extension_id}"
display_name = "Taplo"
file_extensions = ["{extension_id}"]

[[language_servers]]
id = "taplo"
display_name = "Taplo"
language_ids = ["{extension_id}"]
binary_name = "taplo"
args = ["lsp", "stdio"]
"#
    )
}

#[test]
fn a_declared_language_server_needs_the_process_exec_capability() {
    let root = tempfile::tempdir().unwrap();
    install(root.path(), "a-no-permission", &with_server("nopermission", ""));
    install(root.path(), "b-permitted", &with_server("permitted", "\"process_exec\""));

    let mut registry = Registry::new();
    let problems = crate::register_installed(&mut registry, root.path(), &[]);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("process_exec") && problems[0].contains("a language server"), "{problems:?}");
    assert!(registry.language("nopermission").is_none());

    let servers = registry.language_servers_for("permitted");
    assert_eq!(servers.len(), 1, "the permitted extension's server is registered");
    assert_eq!(servers[0].binary_name, "taplo");
    assert_eq!(servers[0].args, vec!["lsp", "stdio"]);
}
