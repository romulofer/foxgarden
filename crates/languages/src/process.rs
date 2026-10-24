//! How an installed extension runs code (`PLAN.md` Track 24 Phase B,
//! executable tier): one short-lived process per question.
//!
//! The extension names a program in its manifest. To ask it something the
//! host starts it, writes one JSON request line to its stdin, closes stdin,
//! and reads one JSON reply from its stdout:
//!
//! ```text
//! -> {"protocol":1,"method":"run_analyzer","params":{...}}
//! <- {"result": ...}            or            {"error": "message"}
//! ```
//!
//! Nothing stays running between questions, so an installed extension costs
//! no idle CPU or memory, and a crashed or hung one cannot leave the editor
//! holding a half-dead connection. The price is a process start per call,
//! which is fine for the questions asked off the keystroke path (running an
//! analyzer, reading a tool's output) and is why per-keystroke questions
//! stay out of this protocol until there is a persistent mode for them.
//!
//! The program runs with the extension directory as its working directory
//! and only with the `process_exec` capability, which the registry checks
//! for every contribution that reaches this module.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The version of the request envelope above. A reply the host cannot read
/// is an error, never a guess.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCommand {
    /// Resolved to inside `dir` at load time.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The extension directory, used as the working directory.
    pub dir: PathBuf,
}

#[derive(Serialize)]
struct Request<'a, P: Serialize> {
    protocol: u32,
    method: &'a str,
    params: &'a P,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply<R> {
    result: Option<R>,
    error: Option<String>,
}

impl ProcessCommand {
    fn spawn(&self) -> Result<std::process::Child, String> {
        let mut attempts_left = 5;
        loop {
            let started = Command::new(&self.program)
                .args(&self.args)
                .current_dir(&self.dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            match started {
                Ok(child) => return Ok(child),
                // "Text file busy": the program was written a moment ago and
                // another thread's `fork` still holds the write handle open
                // (an extension that was just installed). It clears in
                // milliseconds, so retry instead of reporting a failure the
                // user could only answer by trying again.
                Err(e) if e.raw_os_error() == Some(26) && attempts_left > 0 => {
                    attempts_left -= 1;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => return Err(format!("could not start {}: {e}", self.program.display())),
            }
        }
    }

    /// Asks `method`. Blocks until the program exits, so call it from a
    /// background thread (every caller of an analyzer already is on one).
    pub fn call<P: Serialize, R: DeserializeOwned>(&self, method: &str, params: &P) -> Result<R, String> {
        let request = serde_json::to_string(&Request {
            protocol: PROTOCOL_VERSION,
            method,
            params,
        })
        .map_err(|e| format!("could not encode the {method} request: {e}"))?;

        let mut child = self.spawn()?;

        let mut stdin = child.stdin.take().expect("stdin was piped");
        // Writing from a thread: a program that fills its stdout pipe before
        // reading all of a large request would otherwise deadlock with us.
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(request.as_bytes());
            let _ = stdin.write_all(b"\n");
        });
        let mut stderr = child.stderr.take().expect("stderr was piped");
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        let mut output = String::new();
        child
            .stdout
            .take()
            .expect("stdout was piped")
            .read_to_string(&mut output)
            .map_err(|e| format!("could not read the reply to {method}: {e}"))?;
        let status = child.wait().map_err(|e| e.to_string())?;
        let _ = writer.join();
        let stderr = stderr_reader.join().unwrap_or_default();

        if !status.success() {
            let detail = stderr.trim();
            return Err(if detail.is_empty() {
                format!("{} exited with {status}", self.program.display())
            } else {
                format!("{} exited with {status}: {detail}", self.program.display())
            });
        }
        let reply: Reply<R> =
            serde_json::from_str(output.trim()).map_err(|e| format!("unreadable reply to {method}: {e}"))?;
        match (reply.result, reply.error) {
            (_, Some(error)) => Err(error),
            (Some(result), None) => Ok(result),
            (None, None) => Err(format!("the reply to {method} had neither a result nor an error")),
        }
    }
}

/// `relative` resolved against `dir`, refusing anything that leaves it: a
/// manifest is someone else's text, and `../../etc/passwd` or an absolute
/// path is not a file the extension has any business naming.
pub fn inside(dir: &Path, relative: &Path) -> Result<PathBuf, String> {
    let escapes = relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_)));
    if escapes {
        return Err(format!("{} must be a path inside the extension directory", relative.display()));
    }
    Ok(dir.join(relative))
}

#[cfg(test)]
#[path = "process_test.rs"]
mod process_test;
