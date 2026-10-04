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
    /// The tree under `root`. Only `root` itself has to be readable; see
    /// `build_dir` for what happens below it.
    fn build(root: &Path) -> std::io::Result<FileNode> {
        if !root.is_dir() {
            return Ok(FileNode::leaf(root, FileKind::File));
        }
        let canonical = std::fs::canonicalize(root)?;
        let entries = std::fs::read_dir(root)?;
        Ok(FileNode::build_dir(root, entries, &mut vec![canonical]))
    }

    fn leaf(path: &Path, kind: FileKind) -> FileNode {
        FileNode {
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            kind,
            children: Vec::new(),
        }
    }

    /// One directory's subtree. `ancestors` holds the canonical path of
    /// every directory from the root down to this one.
    ///
    /// Never fails: a subdirectory that cannot be read (a root-owned docker
    /// volume, say) shows as an empty folder rather than refusing to open
    /// the whole project. Symlinked directories are followed, but one that
    /// leads back to a directory already being walked (`ln -s .. parent`)
    /// is shown without its children instead of recursing until the stack
    /// overflows.
    fn build_dir(path: &Path, entries: std::fs::ReadDir, ancestors: &mut Vec<PathBuf>) -> FileNode {
        // Sort key is (is_file, path): directories (false) sort before
        // files (true), each group alphabetically by path. `file_type`
        // comes straight off the `DirEntry` rather than a fresh stat call,
        // except for a symlink, which has to be followed to know what it is.
        let mut found: Vec<(bool, bool, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else { continue };
            let child_path = entry.path();
            let is_symlink = file_type.is_symlink();
            let is_dir = if is_symlink { child_path.is_dir() } else { file_type.is_dir() };
            if is_dir
                && child_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| SKIPPED_DIR_NAMES.contains(&name))
            {
                continue;
            }
            found.push((!is_dir, is_symlink, child_path));
        }
        found.sort();

        let parent_canonical = ancestors.last().cloned().unwrap_or_default();
        let children = found
            .into_iter()
            .map(|(is_file, is_symlink, child_path)| {
                if is_file {
                    return FileNode::leaf(&child_path, FileKind::File);
                }
                let canonical = if is_symlink {
                    std::fs::canonicalize(&child_path).ok()
                } else {
                    child_path.file_name().map(|name| parent_canonical.join(name))
                };
                let entries = std::fs::read_dir(&child_path).ok();
                match (canonical, entries) {
                    (Some(canonical), Some(entries)) if !ancestors.contains(&canonical) => {
                        ancestors.push(canonical);
                        let node = FileNode::build_dir(&child_path, entries, ancestors);
                        ancestors.pop();
                        node
                    }
                    _ => FileNode::leaf(&child_path, FileKind::Dir),
                }
            })
            .collect();

        FileNode {
            children,
            ..FileNode::leaf(path, FileKind::Dir)
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
                .position(|child| {
                    (child.kind == FileKind::File, &child.path) > (new_node.kind == FileKind::File, &new_node.path)
                })
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
#[path = "project_test.rs"]
mod project_test;
