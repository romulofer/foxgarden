//! Language server diagnostics that are wrong for the project's layout (a
//! server that treats every Kotlin Multiplatform source set as one module
//! reports each `actual` as a redeclaration), recognized by whichever
//! extension knows that layout rather than by a `match` here.

use std::path::Path;

/// Whether any installed extension says the server's diagnostic (`code`,
/// `message`) on `file` is a false positive. Cheap for a `code` no extension
/// has an opinion on.
pub fn is_spurious_server_diagnostic(project_root: &Path, file: &Path, code: &str, message: &str) -> bool {
    crate::providers::providers()
        .extensions
        .iter()
        .any(|extension| extension.is_spurious_server_diagnostic(project_root, file, code, message))
}
