mod atomic_file;
mod blame;
mod boilerplate;
mod diagnostic;
mod diff;
mod docker;
mod document;
mod drafts;
mod editor_state;
mod file_history;
mod language;
mod profiler;
mod project;
mod project_config;
mod run_config;
mod scaffold;
mod static_analysis;
mod status;
mod text_buffer;

pub use atomic_file::write_atomically;
pub use blame::{BlameLine, GitBlameError, git_blame, parse_porcelain_blame};
pub use boilerplate::generate as generate_boilerplate;
// Build tooling is contributed, not built in (`PLAN.md` Track 24 Phase 5):
// which process compiles a project, where it leaves its classes, what its
// diagnostics look like and where it writes its reports are all answers an
// extension gives through `fg_extension::BuildToolHandle`. Re-exported here
// so the app keeps one import path for editor-facing types.
pub use fg_extension::{
    BuildProblem, BuildTask, BuildToolHandle, ConfigProperty, CoverageStatus, ExtensionHandle, LineCoverage,
    ProblemSeverity, ProjectRelease, RunSpec, ScaffoldSpec, TestCase, TestFailureLocation, TestOutcome, TestSummary,
    summarize,
};
pub use diagnostic::{Diagnostic, Severity};
pub use diff::{
    DiffHunk, DiffLineKind, FileDiff, GitDiffError, RawHunk, git_diff_hunks, git_file_diff, git_file_diff_cached,
    git_show_head, hunk_patch, parse_file_diff, parse_unified_diff,
};
pub use docker::{
    compose_file, container_name, docker_build_command, docker_compose_down_command, docker_compose_up_command,
    docker_run_command, docker_stop_command, has_dockerfile,
};
pub use document::{Document, OpenDocumentError};
pub use drafts::{Draft, discard_draft, pending_drafts, write_draft};
pub use editor_state::{EditorState, TerminalTab};
pub use file_history::{Snapshot, list_snapshots};
pub use language::Language;
pub use profiler::{FlameNode, ProfileEvent, parse_collapsed, profiler_command};
pub use project::{FileKind, FileNode, Project};
pub use project_config::{
    ProjectConfig, load_project_config, parse_project_config, save_project_config, serialize_project_config,
};
pub use run_config::{RunConfig, load_run_configs, parse_run_configs, save_run_configs, serialize_run_configs};
pub use scaffold::write_scaffold;
pub use static_analysis::{
    StaticAnalysisError, checkstyle_diagnostics, line_col_to_byte, pmd_diagnostics, spotbugs_diagnostics,
};
pub use status::{
    GitCommandError, GitStatusError, StatusEntry, git_add, git_apply_cached, git_commit, git_push, git_reset_paths,
    git_status, git_user_first_name,
};
pub use text_buffer::TextBuffer;
