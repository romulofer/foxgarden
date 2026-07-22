use std::fmt;
use std::path::{Path, PathBuf};

use ropey::Rope;

use crate::diagnostic::Diagnostic;
use crate::language::Language;

#[derive(Debug)]
pub enum OpenDocumentError {
    Io(std::io::Error),
    UnsupportedExtension(PathBuf),
}

impl fmt::Display for OpenDocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenDocumentError::Io(e) => write!(f, "{e}"),
            OpenDocumentError::UnsupportedExtension(path) => {
                write!(f, "unsupported extension: {}", path.display())
            }
        }
    }
}

impl std::error::Error for OpenDocumentError {}

impl From<std::io::Error> for OpenDocumentError {
    fn from(e: std::io::Error) -> Self {
        OpenDocumentError::Io(e)
    }
}

pub struct Document {
    pub path: PathBuf,
    pub buffer: Rope,
    pub saved_buffer: Rope,
    pub language: Language,
    pub diagnostics: Vec<Diagnostic>,
}

impl Document {
    pub fn open(path: PathBuf) -> Result<Self, OpenDocumentError> {
        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(Language::from_extension)
            .ok_or_else(|| OpenDocumentError::UnsupportedExtension(path.clone()))?;

        let contents = std::fs::read_to_string(&path)?;
        let buffer = Rope::from_str(&contents);
        let saved_buffer = buffer.clone();

        Ok(Document {
            path,
            buffer,
            saved_buffer,
            language,
            diagnostics: Vec::new(),
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer != self.saved_buffer
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.buffer.to_string())?;
        self.saved_buffer = self.buffer.clone();
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_java_file(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Hello.java");
        std::fs::write(&path, contents).unwrap();
        (dir, path)
    }

    #[test]
    fn edit_marks_document_dirty() {
        let (_dir, path) = temp_java_file("class Hello {}");
        let mut doc = Document::open(path).unwrap();
        assert!(!doc.is_dirty());

        doc.buffer.insert(0, "// comment\n");
        assert!(doc.is_dirty());
    }

    #[test]
    fn save_clears_dirty_state() {
        let (_dir, path) = temp_java_file("class Hello {}");
        let mut doc = Document::open(path.clone()).unwrap();

        doc.buffer.insert(0, "// comment\n");
        assert!(doc.is_dirty());

        doc.save().unwrap();
        assert!(!doc.is_dirty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), doc.buffer.to_string());
    }

    #[test]
    fn edit_undone_to_original_content_is_not_dirty() {
        let (_dir, path) = temp_java_file("class Hello {}");
        let mut doc = Document::open(path).unwrap();

        doc.buffer.insert(0, "// comment\n");
        assert!(doc.is_dirty());

        doc.buffer.remove(0.."// comment\n".len());
        assert!(!doc.is_dirty());
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("readme.txt");
        std::fs::write(&path, "hello").unwrap();

        let result = Document::open(path);
        assert!(matches!(result, Err(OpenDocumentError::UnsupportedExtension(_))));
    }
}
