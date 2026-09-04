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
const SKIPPED_DIR_NAMES: &[&str] = &[
    // This app's own per-project state — history snapshots, drafts, run
    // configs. It's bookkeeping about the project, not part of it, and
    // showing it invites editing files the editor rewrites underneath.
    ".foxgarden",
    ".git",
    "target",
    "node_modules",
    "build",
    ".idea",
    "dist",
    "out",
    ".svn",
    ".hg",
];

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

    /// Every directory in the tree, root included — what the app watches
    /// for filesystem changes so the side panel notices a file created or
    /// deleted outside FoxGarden.
    ///
    /// Derived from the already-built tree rather than a fresh walk, which
    /// means it inherits `SKIPPED_DIR_NAMES` for free: watching `target/`
    /// or `.git/` would register thousands of watches for churn nobody
    /// wants shown in the tree anyway.
    pub fn directories(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        collect_directories(&self.tree, &mut dirs);
        dirs
    }
}

impl Project {
    /// Drops `path` (and everything under it) from the in-memory tree, so a
    /// delete or a move shows up immediately instead of waiting for the
    /// next full walk. Returns whether anything was actually removed.
    ///
    /// This is the cheap half of keeping the tree honest: the app also
    /// schedules a real (background) re-walk, but that lands a frame or
    /// more later, and a file the user just deleted must not still be
    /// sitting there in the meantime.
    pub fn remove_path(&mut self, path: &Path) -> bool {
        remove_from(&mut self.tree, path)
    }

    /// Inserts `path` into the tree in its correct sorted position,
    /// creating any missing parent directory nodes along the way.
    /// The counterpart to `remove_path` for a file/folder that was just
    /// created (or moved into place). Returns whether anything changed.
    pub fn insert_path(&mut self, path: &Path, kind: FileKind) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let components: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if components.is_empty() {
            return false;
        }
        insert_into(&mut self.tree, &components, kind)
    }
}

fn remove_from(node: &mut FileNode, path: &Path) -> bool {
    if node.kind != FileKind::Dir {
        return false;
    }
    if let Some(index) = node.children.iter().position(|child| child.path == path) {
        node.children.remove(index);
        return true;
    }
    node.children
        .iter_mut()
        .filter(|child| path.starts_with(&child.path))
        .any(|child| remove_from(child, path))
}

fn insert_into(node: &mut FileNode, components: &[String], kind: FileKind) -> bool {
    let Some((name, rest)) = components.split_first() else {
        return false;
    };
    let child_path = node.path.join(name);
    let last = rest.is_empty();
    let child_kind = if last { kind } else { FileKind::Dir };

    let existing = node.children.iter().position(|child| child.path == child_path);
    let index = match existing {
        Some(index) => index,
        None => {
            let new_node = FileNode {
                path: child_path,
                name: name.clone(),
                kind: child_kind,
                children: Vec::new(),
            };
            // Same ordering `FileNode::build` produces: directories first,
            // then files, each group alphabetically — so an inserted entry
            // lands where a rebuilt tree would have put it.
            let position = node
                .children
                .iter()
                .position(|child| (child.kind == FileKind::File, &child.path) > (new_node.kind == FileKind::File, &new_node.path))
                .unwrap_or(node.children.len());
            node.children.insert(position, new_node);
            if last {
                return true;
            }
            position
        }
    };
    if last {
        return existing.is_none();
    }
    insert_into(&mut node.children[index], rest, kind)
}

fn collect_directories(node: &FileNode, into: &mut Vec<PathBuf>) {
    if node.kind != FileKind::Dir {
        return;
    }
    into.push(node.path.clone());
    for child in &node.children {
        collect_directories(child, into);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_path_drops_a_file_and_a_whole_subtree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/main")).unwrap();
        std::fs::write(root.join("src/main/Main.java"), "class Main {}").unwrap();
        std::fs::write(root.join("README.md"), "").unwrap();
        let mut project = Project::open(root.to_path_buf()).unwrap();

        assert!(project.remove_path(&root.join("src/main/Main.java")));
        assert!(!project.directories().is_empty());
        assert!(project.remove_path(&root.join("src")));
        assert!(!project.directories().contains(&root.join("src/main")));
        assert!(!project.remove_path(&root.join("does/not/exist")));
    }

    #[test]
    fn insert_path_lands_where_a_rebuilt_tree_would_have_put_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("README.md"), "").unwrap();
        let mut project = Project::open(root.to_path_buf()).unwrap();

        std::fs::write(root.join("src/New.java"), "class New {}").unwrap();
        assert!(project.insert_path(&root.join("src/New.java"), FileKind::File));

        let rebuilt = Project::open(root.to_path_buf()).unwrap();
        assert_eq!(project, rebuilt, "an inserted node must match a full rebuild");
    }

    #[test]
    fn insert_path_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut project = Project::open(root.to_path_buf()).unwrap();

        std::fs::create_dir_all(root.join("src/main/java")).unwrap();
        std::fs::write(root.join("src/main/java/A.java"), "class A {}").unwrap();
        assert!(project.insert_path(&root.join("src/main/java/A.java"), FileKind::File));

        assert_eq!(project, Project::open(root.to_path_buf()).unwrap());
    }

    #[test]
    fn insert_path_ignores_anything_outside_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::open(dir.path().to_path_buf()).unwrap();
        assert!(!project.insert_path(Path::new("/somewhere/else/A.java"), FileKind::File));
    }

    #[test]
    fn directories_lists_every_directory_and_skips_the_ignored_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/main/java")).unwrap();
        std::fs::create_dir_all(root.join("target/classes")).unwrap();
        std::fs::write(root.join("src/main/java/Main.java"), "class Main {}").unwrap();

        let project = Project::open(root.to_path_buf()).unwrap();
        let dirs = project.directories();

        assert!(dirs.contains(&root.to_path_buf()));
        assert!(dirs.contains(&root.join("src/main/java")));
        assert!(!dirs.iter().any(|d| d.starts_with(root.join("target"))));
    }

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
        // This app's own state directory: history snapshots and drafts are
        // bookkeeping about the project, not files to browse.
        std::fs::create_dir_all(root.join(".foxgarden/drafts")).unwrap();
        std::fs::write(root.join(".foxgarden/drafts/Main.java.draft"), "").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/Main.java"), "class Main {}").unwrap();

        let project = Project::open(root.to_path_buf()).unwrap();

        let names: Vec<&str> = project.tree.children.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["src"], "only src should remain in the tree");
    }
}
