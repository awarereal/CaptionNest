//! Running bundled command-line tools: hidden console windows on Windows, killed on
//! cancellation, output streamed line by line.

use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult, ErrorCode};

/// Builds a command for a bundled tool with stdin closed and no console window.
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // Tools must never depend on the user's environment (e.g. a different ffmpeg on PATH).
    if let Some(dir) = program.parent() {
        cmd.current_dir(dir);
    }
    cmd
}

/// Like `command`, for a program that starts programs of its own (yt-dlp runs ffmpeg and
/// Deno): it gets its own process group, so stopping it stops them too.
pub fn command_tree(program: &Path) -> Command {
    let mut cmd = command(program);
    #[cfg(unix)]
    cmd.process_group(0);
    cmd
}

/// Stops a process and everything it started. On macOS that's its process group (only
/// set up by `command_tree`; for other processes the group kill finds nothing and the
/// plain kill below does the work); on Windows, the process tree.
async fn kill_tree(child: &mut Child) {
    if let Some(pid) = child.id() {
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("/bin/kill").args(["-KILL", "--", &format!("-{pid}")]).stderr(Stdio::null()).status();
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let _ = std::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    let _ = child.kill().await;
}

pub struct Output {
    pub status: std::process::ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr_tail: String,
}

/// Spawns `cmd`, calls `on_stdout_line` / `on_stderr_line` for every line, and waits.
/// Kills the process if `cancel` fires. Keeps stdout bytes when `keep_stdout` is set,
/// and the last ~8 KB of stderr for error details.
pub async fn run_streaming<F, G>(
    mut cmd: Command,
    cancel: &CancellationToken,
    keep_stdout: bool,
    mut on_stdout_line: F,
    mut on_stderr_line: G,
) -> AppResult<Output>
where
    F: FnMut(&str),
    G: FnMut(&str),
{
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let program = format!("{:?}", cmd.as_std().get_program());
    let mut child: Child = cmd.spawn().map_err(|e| {
        AppError::new(ErrorCode::ToolMissing, "Part of CaptionNest could not start. Please reinstall the app.")
            .with_detail(format!("{program}: {e}"))
    })?;

    let mut out = LineReader::new(child.stdout.take().expect("piped stdout"));
    let mut err = LineReader::new(child.stderr.take().expect("piped stderr"));
    let mut stdout_bytes = Vec::new();
    let mut stderr_tail = String::new();
    let (mut out_done, mut err_done) = (false, false);

    while !(out_done && err_done) {
        tokio::select! {
            _ = cancel.cancelled() => {
                kill_tree(&mut child).await;
                return Err(AppError::cancelled());
            }
            line = out.next(), if !out_done => match line {
                Some(bytes) => {
                    if keep_stdout { stdout_bytes.extend_from_slice(&bytes); stdout_bytes.push(b'\n'); }
                    on_stdout_line(&String::from_utf8_lossy(&bytes));
                }
                None => out_done = true,
            },
            line = err.next(), if !err_done => match line {
                Some(bytes) => {
                    let s = String::from_utf8_lossy(&bytes);
                    on_stderr_line(&s);
                    stderr_tail.push_str(&s);
                    stderr_tail.push('\n');
                    if stderr_tail.len() > 16_384 {
                        let cut = stderr_tail.len() - 8_192;
                        let cut = (cut..stderr_tail.len()).find(|i| stderr_tail.is_char_boundary(*i)).unwrap_or(0);
                        stderr_tail.drain(..cut);
                    }
                }
                None => err_done = true,
            },
        }
    }
    let status = tokio::select! {
        _ = cancel.cancelled() => {
            kill_tree(&mut child).await;
            return Err(AppError::cancelled());
        }
        status = child.wait() => status?,
    };
    Ok(Output { status, stdout: stdout_bytes, stderr_tail })
}

/// Reads lines terminated by `\n` or `\r` (FFmpeg and whisper use `\r` for live updates).
struct LineReader<R> {
    reader: BufReader<R>,
}

impl<R: AsyncRead + Unpin> LineReader<R> {
    fn new(r: R) -> Self {
        Self { reader: BufReader::new(r) }
    }

    async fn next(&mut self) -> Option<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            let buf = match self.reader.fill_buf().await {
                Ok(b) => b,
                Err(_) => return None,
            };
            if buf.is_empty() {
                return if line.is_empty() { None } else { Some(line) };
            }
            if let Some(pos) = buf.iter().position(|b| *b == b'\n' || *b == b'\r') {
                line.extend_from_slice(&buf[..pos]);
                self.reader.consume(pos + 1);
                return Some(line);
            }
            let n = buf.len();
            line.extend_from_slice(buf);
            self.reader.consume(n);
        }
    }
}
