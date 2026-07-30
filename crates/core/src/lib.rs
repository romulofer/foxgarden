mod blame;
mod boilerplate;
mod diagnostic;
mod diff;
mod document;
mod editor_state;
mod gradle;
mod language;
mod maven;
mod project;
mod run_config;
mod spring_config_metadata;
mod static_analysis;
mod status;

pub use blame::{BlameLine, GitBlameError, git_blame, parse_porcelain_blame};
pub use boilerplate::generate as generate_boilerplate;
pub use diagnostic::{Diagnostic, Severity};
pub use diff::{DiffHunk, DiffLineKind, GitDiffError, git_diff_hunks, parse_unified_diff};
pub use document::{Document, OpenDocumentError};
pub use editor_state::{EditorState, TerminalTab};
pub use gradle::{GradleClasspath, GradleDependency, GradleError, GradleProject, gradle_classpaths, gradle_projects};
pub use language::Language;
pub use maven::{MavenClasspathError, MavenDependency, MavenParent, MavenProject, maven_classpath, parse_pom};
pub use project::{FileKind, FileNode, Project};
pub use run_config::{RunConfig, load_run_configs, parse_run_configs, save_run_configs, serialize_run_configs};
pub use spring_config_metadata::{
    SpringConfigProperty, parse_metadata_json, scan_classpath_for_metadata, scan_jar_for_metadata,
};
pub use static_analysis::{StaticAnalysisError, checkstyle_diagnostics, pmd_diagnostics};
pub use status::{
    GitCommandError, GitStatusError, StatusEntry, git_add, git_commit, git_reset_paths, git_status,
    git_user_first_name,
};
