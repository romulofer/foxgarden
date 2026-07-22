use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Dir,
}

/// Directory names that are never useful to browse in a Java/Kotlin/Maven
/// project and can be enormous — VCS internals, build output, installed
/// dependencies. Skipped without ever being `read_dir`'d, so a `.git` with
/// tens of thousands of loose objects or a populated `node_modules` doesn't
/// turn opening the project into a multi-second walk of files nobody wants
/// to see in the tree anyway.
const SKIPPED_DIR_NAMES: &[&str] =
    &[".git", "target", "node_modules", "build", ".idea", "dist", "out", ".svn", ".hg"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileKind,
    pub children: Vec<FileNode>,
}

impl FileNode {
    fn build(path: &Path) -> std::io::Result<FileNode> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if path.is_dir() {
            // Sort key is (is_file, path): directories (false) sort before
            // files (true), each group alphabetically by path. `file_type`
            // comes straight off the `DirEntry` rather than a fresh
            // `path.is_dir()` stat call.
            let mut entries: Vec<(bool, PathBuf)> = Vec::new();
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let is_dir = entry.file_type()?.is_dir();
                let child_path = entry.path();
                if is_dir
                    && child_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|name| SKIPPED_DIR_NAMES.contains(&name))
                {
                    continue;
                }
                entries.push((!is_dir, child_path));
            }
            entries.sort();

            let children = entries
                .iter()
                .map(|(_, child_path)| FileNode::build(child_path))
                .collect::<std::io::Result<Vec<_>>>()?;

            Ok(FileNode {
                path: path.to_path_buf(),
                name,
                kind: FileKind::Dir,
                children,
            })
        } else {
            Ok(FileNode {
                path: path.to_path_buf(),
                name,
                kind: FileKind::File,
                children: Vec::new(),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub root: PathBuf,
    pub tree: FileNode,
}

impl Project {
    pub fn open(root: PathBuf) -> std::io::Result<Self> {
        let tree = FileNode::build(&root)?;
        Ok(Project { root, tree })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_nested_tree_with_empty_dirs_and_mixed_file_types() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("src/main/java")).unwrap();
        std::fs::create_dir_all(root.join("empty_dir")).unwrap();
        std::fs::write(root.join("src/main/java/Main.java"), "class Main {}").unwrap();
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();

        let project = Project::open(root.to_path_buf()).unwrap();
        assert_eq!(project.root, root);
        assert_eq!(project.tree.kind, FileKind::Dir);

        // dirs before files, each group sorted: empty_dir, src, pom.xml
        let names: Vec<&str> = project.tree.children.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["empty_dir", "src", "pom.xml"]);

        let empty_dir = &project.tree.children[0];
        assert_eq!(empty_dir.kind, FileKind::Dir);
        assert!(empty_dir.children.is_empty());

        let src = &project.tree.children[1];
        assert_eq!(src.kind, FileKind::Dir);
        let main = &src.children[0];
        assert_eq!(main.name, "main");
        let java = &main.children[0];
        assert_eq!(java.name, "java");
        let main_java = &java.children[0];
        assert_eq!(main_java.name, "Main.java");
        assert_eq!(main_java.kind, FileKind::File);

        let pom = &project.tree.children[2];
        assert_eq!(pom.kind, FileKind::File);
        assert!(pom.children.is_empty());
    }

    #[test]
    fn skips_git_target_and_node_modules_without_descending_into_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        std::fs::write(root.join(".git/objects/deadbeef"), "").unwrap();
        std::fs::create_dir_all(root.join("target/classes")).unwrap();
        std::fs::write(root.join("target/classes/Main.class"), "").unwrap();
        std::fs::create_dir_all(root.join("node_modules/some-pkg")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/Main.java"), "class Main {}").unwrap();

        let project = Project::open(root.to_path_buf()).unwrap();

        let names: Vec<&str> = project.tree.children.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["src"], "only src should remain in the tree");
    }
}
