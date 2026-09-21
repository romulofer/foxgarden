use std::path::Path;

use crate::language::Language;

/// Generates starter content for a newly created `.java`/`.kt` file: a
/// class declaration named after the file, plus a `package` line inferred
/// from the file's location under a conventional Maven/Gradle source root
/// (`src/main/java`, `src/main/kotlin`, `src/test/java`, `src/test/kotlin`).
/// If the file doesn't sit under one of those, no `package` line is emitted
/// — there's nothing sane to infer outside that convention.
pub fn generate(language: Language, project_root: &Path, file_path: &Path) -> String {
    let class_name = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("Main");
    let package = infer_package(project_root, file_path);

    match language {
        Language::Java => {
            let mut out = String::new();
            if let Some(package) = &package {
                out.push_str(&format!("package {package};\n\n"));
            }
            out.push_str(&format!("public class {class_name} {{\n\n}}\n"));
            out
        }
        Language::Kotlin => {
            let mut out = String::new();
            if let Some(package) = &package {
                out.push_str(&format!("package {package}\n\n"));
            }
            out.push_str(&format!("class {class_name} {{\n\n}}\n"));
            out
        }
        // Class/package scaffolding doesn't mean anything for a config or
        // markup file, and this build cannot know what it would mean for a
        // language contributed by an extension it has never seen — either
        // way a new file just starts empty, same as one with no recognized
        // language at all. Generating *something* for an unknown language
        // would be strictly worse than generating nothing: a Java class
        // body in a file of some other language is not a helpful guess.
        _ => String::new(),
    }
}

fn infer_package(project_root: &Path, file_path: &Path) -> Option<String> {
    let relative = file_path.strip_prefix(project_root).ok()?;
    let dir = relative.parent()?;
    let components: Vec<&str> = dir.components().filter_map(|c| c.as_os_str().to_str()).collect();

    let anchor = components
        .windows(3)
        .position(|w| w[0] == "src" && (w[1] == "main" || w[1] == "test") && (w[2] == "java" || w[2] == "kotlin"))?;
    let package_components = &components[anchor + 3..];

    if package_components.is_empty() {
        None
    } else {
        Some(package_components.join("."))
    }
}

#[cfg(test)]
#[path = "boilerplate_test.rs"]
mod boilerplate_test;
