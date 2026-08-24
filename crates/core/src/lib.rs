mod blame;
mod boilerplate;
mod build_output;
mod coverage;
mod diagnostic;
mod diff;
mod docker;
mod document;
mod editor_state;
mod file_history;
mod gradle;
mod java_release;
mod language;
mod maven;
mod project;
mod project_config;
mod run;
mod run_config;
mod scaffold;
mod spring_config_metadata;
mod static_analysis;
mod status;
mod test_report;

pub use blame::{BlameLine, GitBlameError, git_blame, parse_porcelain_blame};
pub use boilerplate::generate as generate_boilerplate;
pub use build_output::{BuildProblem, build_command, default_classes_dir, detect_build_tool, parse_build_output_line};
pub use coverage::{CoverageStatus, LineCoverage, coverage_command, coverage_report_path, parse_jacoco_xml, resolve_coverage_paths};
pub use diagnostic::{Diagnostic, Severity};
pub use diff::{
    DiffHunk, DiffLineKind, FileDiff, GitDiffError, RawHunk, git_diff_hunks, git_file_diff, git_file_diff_cached,
    git_show_head, hunk_patch, parse_file_diff, parse_unified_diff,
};
pub use docker::{compose_file, docker_build_command, docker_compose_up_command, docker_run_command, has_dockerfile};
pub use document::{Document, OpenDocumentError};
pub use editor_state::{EditorState, TerminalTab};
pub use file_history::{Snapshot, list_snapshots};
pub use gradle::{GradleClasspath, GradleDependency, GradleError, GradleProject, gradle_classpaths, gradle_projects};
pub use java_release::{
    JavaRelease, build_files as java_release_build_files, detect as detect_java_release, parse_release_token,
    release_from_gradle, release_from_pom, release_from_version_file,
};
pub use language::Language;
pub use maven::{MavenClasspathError, MavenDependency, MavenParent, MavenProject, maven_classpath, parse_pom};
pub use project::{FileKind, FileNode, Project};
pub use project_config::{
    ProjectConfig, load_project_config, parse_project_config, save_project_config, serialize_project_config,
};
pub use run::{RunSetupError, resolve_classpath, run_command};
pub use run_config::{RunConfig, load_run_configs, parse_run_configs, save_run_configs, serialize_run_configs};
pub use scaffold::{BuildTool, ProjectLanguage, ScaffoldSpec, scaffold_files, write_scaffold};
pub use spring_config_metadata::{
    SpringConfigProperty, parse_metadata_json, scan_classpath_for_metadata, scan_jar_for_metadata,
};
pub use static_analysis::{StaticAnalysisError, checkstyle_diagnostics, line_col_to_byte, pmd_diagnostics, spotbugs_diagnostics};
pub use status::{
    GitCommandError, GitStatusError, StatusEntry, git_add, git_apply_cached, git_commit, git_push, git_reset_paths,
    git_status, git_user_first_name,
};
pub use test_report::{
    TestCase, TestOutcome, TestSummary, failure_line, parse_junit_xml, scan_test_reports, summarize, test_command,
    test_source_file,
};
