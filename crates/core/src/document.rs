use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use ropey::Rope;

use crate::diagnostic::Diagnostic;
use crate::language::Language;

#[derive(Debug)]
pub enum OpenDocumentError {
    Io(PathBuf, std::io::Error),
    Binary(PathBuf),
}

impl fmt::Display for OpenDocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenDocumentError::Io(path, e) => write!(f, "{}: {e}", path.display()),
            OpenDocumentError::Binary(path) => write!(f, "not a text file: {}", path.display()),
        }
    }
}

impl std::error::Error for OpenDocumentError {}

pub struct Document {
    pub path: PathBuf,
    pub buffer: Rope,
    pub saved_buffer: Rope,
    /// `None` for files whose extension isn't a recognized language (or has
    /// none at all) — the file still opens and edits like any other, it
    /// just gets no syntax highlighting, diagnostics, or (in the future)
    /// completion.
    pub language: Option<Language>,
    pub diagnostics: Vec<Diagnostic>,
    /// Secondary Ctrl+D cursors/selections, as **char** (not byte) index
    /// ranges into `buffer`. An empty range is a bare caret. The primary
    /// cursor/selection remains owned by the editor widget's own state;
    /// this only tracks the extras layered on top of it.
    pub extra_selections: Vec<Range<usize>>,
    /// User-toggled "don't let me edit this by accident" flag — session-local
    /// only (not persisted; a fresh open always starts editable), same as
    /// `zen_mode`. Blocks every edit path in `widgets::editor::show`, not
    /// just direct typing; see that function's `strip_mutating_events`.
    pub read_only: bool,
}

impl Document {
    pub fn open(path: PathBuf) -> Result<Self, OpenDocumentError> {
        // Falls back to a bare-file-name check (`from_filename`) only when
        // there's no extension-based match — a plain `Dockerfile` has no
        // extension at all for `from_extension` to key off, but something
        // like `notes.dockerfile.bak` should still lose to whatever
        // `from_extension` says about its actual (`bak`) extension, not be
        // second-guessed by the name check.
        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(Language::from_extension)
            .or_else(|| path.file_name().and_then(|n| n.to_str()).and_then(Language::from_filename));

        // The side panel lets any file in the tree be clicked — including
        // build artifacts (`target/*.class`, jars) and binary assets, since
        // there's no ignore list — and doesn't guess from the extension
        // alone whether a file is text. Without this, a click on a large
        // binary would fall straight into `read_to_string` below: a full
        // read plus a whole-buffer UTF-8 validation pass, on the UI thread,
        // just to discover it isn't text. Sniffing a small prefix for a NUL
        // byte (the standard binary heuristic) rejects those files fast
        // instead of blocking on a multi-megabyte read.
        match looks_binary(&path) {
            Ok(true) => return Err(OpenDocumentError::Binary(path)),
            Ok(false) => {}
            Err(e) => return Err(OpenDocumentError::Io(path, e)),
        }

        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(e) => return Err(OpenDocumentError::Io(path, e)),
        };
        let buffer = Rope::from_str(&contents);
        let saved_buffer = buffer.clone();

        Ok(Document {
            path,
            buffer,
            saved_buffer,
            language,
            diagnostics: Vec::new(),
            extra_selections: Vec::new(),
            read_only: false,
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer != self.saved_buffer
    }

    /// Trims trailing whitespace from every line before writing, and updates
    /// `buffer` itself (not just the bytes written to disk) to match — so
    /// the editor immediately shows what's actually on disk, and `is_dirty`
    /// (derived from `buffer != saved_buffer`) doesn't flip back to `true`
    /// right after a save because the two silently diverged.
    pub fn save(&mut self) -> std::io::Result<()> {
        let trimmed = trim_trailing_whitespace(&self.buffer.to_string());
        self.buffer = Rope::from_str(&trimmed);
        std::fs::write(&self.path, &trimmed)?;
        self.saved_buffer = self.buffer.clone();
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Strips trailing spaces/tabs from every line, preserving line count,
/// line-ending style, and whether the text ends with a trailing newline. A
/// `\r\n`-terminated line has its `\r` set aside first and reattached after
/// trimming — otherwise `\r` (not itself whitespace we want to strip) would
/// block `trim_end_matches` from reaching the spaces/tabs before it.
fn trim_trailing_whitespace(text: &str) -> String {
    text.split('\n')
        .map(|line| match line.strip_suffix('\r') {
            Some(content) => format!("{}\r", content.trim_end_matches([' ', '\t'])),
            None => line.trim_end_matches([' ', '\t']).to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Reads only the first `SNIFF_LEN` bytes of `path` and checks for a NUL
/// byte. Deliberately a prefix, not the whole file: the point is to bound
/// the cost of rejecting a binary to a few KB instead of its full size.
fn looks_binary(path: &Path) -> std::io::Result<bool> {
    use std::io::Read;

    const SNIFF_LEN: usize = 8192;

    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; SNIFF_LEN];
    let read = file.read(&mut buf)?;
    Ok(buf[..read].contains(&0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_marks_document_dirty() {
        let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
        let mut doc = Document::open(path).unwrap();
        assert!(!doc.is_dirty());

        doc.buffer.insert(0, "// comment\n");
        assert!(doc.is_dirty());
    }

    #[test]
    fn save_clears_dirty_state() {
        let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
        let mut doc = Document::open(path.clone()).unwrap();

        doc.buffer.insert(0, "// comment\n");
        assert!(doc.is_dirty());

        doc.save().unwrap();
        assert!(!doc.is_dirty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), doc.buffer.to_string());
    }

    #[test]
    fn edit_undone_to_original_content_is_not_dirty() {
        let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
        let mut doc = Document::open(path).unwrap();

        doc.buffer.insert(0, "// comment\n");
        assert!(doc.is_dirty());

        doc.buffer.remove(0.."// comment\n".len());
        assert!(!doc.is_dirty());
    }

    #[test]
    fn unrecognized_extension_opens_as_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("readme.txt");
        std::fs::write(&path, "hello").unwrap();

        let doc = Document::open(path).unwrap();
        assert_eq!(doc.language, None);
        assert_eq!(doc.buffer.to_string(), "hello");
    }

    #[test]
    fn extensionless_file_opens_as_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("README");
        std::fs::write(&path, "hello").unwrap();

        let doc = Document::open(path).unwrap();
        assert_eq!(doc.language, None);
    }

    #[test]
    fn save_trims_trailing_whitespace_from_every_line() {
        let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
        let mut doc = Document::open(path.clone()).unwrap();

        doc.buffer = Rope::from_str("class Hello {   \n\tint x;\t\t\n}   \n");
        doc.save().unwrap();

        let expected = "class Hello {\n\tint x;\n}\n";
        assert_eq!(doc.buffer.to_string(), expected);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        assert!(!doc.is_dirty(), "buffer and saved_buffer must agree right after save");
    }

    #[test]
    fn save_trims_a_final_line_with_no_trailing_newline() {
        let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
        let mut doc = Document::open(path).unwrap();

        doc.buffer = Rope::from_str("class Hello {}  ");
        doc.save().unwrap();

        // No newline should be added where there wasn't one.
        assert_eq!(doc.buffer.to_string(), "class Hello {}");
    }

    #[test]
    fn save_preserves_crlf_line_endings_while_trimming() {
        let (_dir, path) = test_support::temp_file("Hello.java", "class Hello {}");
        let mut doc = Document::open(path).unwrap();

        doc.buffer = Rope::from_str("class Hello {}  \r\n  int x;\r\n");
        doc.save().unwrap();

        assert_eq!(doc.buffer.to_string(), "class Hello {}\r\n  int x;\r\n");
    }

    #[test]
    fn binary_file_is_rejected_without_reading_it_fully() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("icon.png");
        // A NUL a few bytes into an otherwise huge file: if `open` fell
        // through to `read_to_string`, this would still succeed in reading
        // (then fail UTF-8 validation) — the point of `looks_binary` is to
        // catch it from the first 8KB alone, before that full read happens.
        let mut contents = vec![0x89, b'P', b'N', b'G', 0x00];
        contents.extend(std::iter::repeat_n(b'a', 50 * 1024 * 1024));
        std::fs::write(&path, &contents).unwrap();

        let result = Document::open(path.clone());
        assert!(matches!(result, Err(OpenDocumentError::Binary(p)) if p == path));
    }
}
