use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Dir,
}

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
            let mut entries: Vec<PathBuf> = std::fs::read_dir(path)?
                .map(|entry| entry.map(|e| e.path()))
                .collect::<std::io::Result<_>>()?;
            entries.sort();

            let children = entries
                .iter()
                .map(|child_path| FileNode::build(child_path))
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

        // top level, sorted: empty_dir, pom.xml, src
        let names: Vec<&str> = project.tree.children.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["empty_dir", "pom.xml", "src"]);

        let empty_dir = &project.tree.children[0];
        assert_eq!(empty_dir.kind, FileKind::Dir);
        assert!(empty_dir.children.is_empty());

        let pom = &project.tree.children[1];
        assert_eq!(pom.kind, FileKind::File);
        assert!(pom.children.is_empty());

        let src = &project.tree.children[2];
        assert_eq!(src.kind, FileKind::Dir);
        let main = &src.children[0];
        assert_eq!(main.name, "main");
        let java = &main.children[0];
        assert_eq!(java.name, "java");
        let main_java = &java.children[0];
        assert_eq!(main_java.name, "Main.java");
        assert_eq!(main_java.kind, FileKind::File);
    }
}
