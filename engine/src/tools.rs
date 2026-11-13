//! Running the external tools: yt-dlp (probe), ffmpeg (merge / convert), and the startup check.

use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::EngineError;
use crate::ytdlp::classify_ytdlp_error;

/// Windows process flag: don't open a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command with no console window, piped output, and killed if the handle is dropped.
pub fn command(program: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Runs `yt-dlp -J --no-playlist <url>` and returns its JSON output.
pub async fn run_ytdlp_probe(url: &str) -> Result<String, EngineError> {
    let output = command("yt-dlp")
        .args(["-J", "--no-playlist", url])
        .output()
        .await
        .map_err(|e| missing_or_io(e, "yt-dlp"))?;
    if !output.status.success() {
        return Err(classify_ytdlp_error(&String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn missing_or_io(e: std::io::Error, tool: &str) -> EngineError {
    if e.kind() == ErrorKind::NotFound {
        EngineError::ToolMissing(tool.to_string())
    } else {
        EngineError::Io(e)
    }
}

fn base_args() -> Vec<OsString> {
    // Quiet output; -n = never overwrite an existing file.
    ["-hide_banner", "-loglevel", "error", "-n"].iter().map(OsString::from).collect()
}

/// Copies the video and audio streams into one file without re-encoding.
pub fn merge_args(video: &Path, audio: &Path, out: &Path) -> Vec<OsString> {
    let mut a = base_args();
    a.extend(["-i".into(), video.into(), "-i".into(), audio.into()]);
    a.extend(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"].iter().map(OsString::from));
    a.push(out.into());
    a
}

/// Converts audio to MP3 at the highest VBR quality.
pub fn mp3_args(audio: &Path, out: &Path) -> Vec<OsString> {
    let mut a = base_args();
    a.extend(["-i".into(), audio.into()]);
    a.extend(["-vn", "-c:a", "libmp3lame", "-q:a", "0"].iter().map(OsString::from));
    a.push(out.into());
    a
}

/// Runs ffmpeg with `args`. On cancel, kills ffmpeg and waits for it to exit.
pub async fn run_ffmpeg(args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), EngineError> {
    let mut child = command("ffmpeg")
        .args(&args)
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| missing_or_io(e, "ffmpeg"))?;

    // Read stderr in the background so ffmpeg never blocks on a full pipe.
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).into_owned()
    });

    let status = tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.kill().await; // kill() also waits for the process to exit
            return Err(EngineError::Cancelled);
        }
        s = child.wait() => s?,
    };
    if status.success() {
        return Ok(());
    }
    let err = stderr_task.await.unwrap_or_default();
    let last = err
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .last()
        .map(str::to_string)
        .unwrap_or_else(|| format!("exit status {status}"));
    Err(EngineError::Ffmpeg(last))
}

/// True if `program version_arg` runs and exits successfully.
pub async fn tool_available(program: &str, version_arg: &str) -> bool {
    matches!(command(program).arg(version_arg).output().await, Ok(o) if o.status.success())
}

/// Names of the required tools that are missing, in the order yt-dlp, ffmpeg, deno.
pub async fn missing_tools() -> Vec<&'static str> {
    let mut missing = Vec::new();
    for (tool, arg) in [("yt-dlp", "--version"), ("ffmpeg", "-version"), ("deno", "--version")] {
        if !tool_available(tool, arg).await {
            missing.push(tool);
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn merge_args_exact() {
        let a = merge_args(Path::new("v.part"), Path::new("a.part"), Path::new("out.mkv"));
        let a: Vec<_> = a.iter().map(|s| s.to_str().unwrap()).collect();
        assert_eq!(
            a,
            ["-hide_banner", "-loglevel", "error", "-n", "-i", "v.part", "-i", "a.part", "-map", "0:v:0", "-map", "1:a:0", "-c", "copy", "out.mkv"]
        );
    }

    #[test]
    fn mp3_args_exact() {
        let a = mp3_args(Path::new("a.part"), Path::new("out.mp3"));
        let a: Vec<_> = a.iter().map(|s| s.to_str().unwrap()).collect();
        assert_eq!(
            a,
            ["-hide_banner", "-loglevel", "error", "-n", "-i", "a.part", "-vn", "-c:a", "libmp3lame", "-q:a", "0", "out.mp3"]
        );
    }

    #[tokio::test]
    async fn unknown_tool_is_unavailable() {
        assert!(!tool_available("definitely-not-a-real-tool-xyz", "--version").await);
    }

    #[tokio::test]
    #[ignore = "needs ffmpeg on PATH"]
    async fn ffmpeg_cancel_kills_process() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("long.mp3");
        let args: Vec<OsString> = ["-f", "lavfi", "-i", "anullsrc", "-t", "600", "-c:a", "libmp3lame"]
            .iter()
            .map(OsString::from)
            .chain([out.into_os_string()])
            .collect();
        let cancel = CancellationToken::new();
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            c.cancel();
        });

        let started = Instant::now();
        let r = run_ffmpeg(args, &cancel).await;
        assert!(matches!(r, Err(EngineError::Cancelled)), "got {r:?}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
