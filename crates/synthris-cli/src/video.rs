use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
#[derive(Debug, Clone, Copy)]
pub enum VideoCodec {
    Mp4H264,
}

#[derive(Debug, Clone, Copy)]
pub struct VideoEncodeOptions {
    pub fps: u32,
    pub codec: VideoCodec,
}

pub fn encode_raw_stream<F>(
    output_path: &Path,
    options: VideoEncodeOptions,
    width: u32,
    height: u32,
    write_frames: F,
) -> Result<()>
where
    F: FnOnce(&mut dyn Write) -> Result<()>,
{
    let fps = options.fps.max(1);

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgb24")
        .arg("-s")
        .arg(format!("{}x{}", width, height))
        .arg("-r")
        .arg(fps.to_string())
        .arg("-i")
        .arg("pipe:0")
        .arg("-an")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-movflags")
        .arg("+faststart");

    match options.codec {
        VideoCodec::Mp4H264 => {
            cmd.arg("-c:v").arg("libx264");
        }
    }

    cmd.arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| "failed to spawn ffmpeg. Install ffmpeg and ensure it is on PATH.")?;

    let write_result = (|| -> Result<()> {
        let stdin = child
            .stdin
            .as_mut()
            .context("failed to open ffmpeg stdin")?;
        write_frames(stdin)
    })();

    let output = child
        .wait_with_output()
        .context("failed waiting for ffmpeg process")?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    if let Err(err) = write_result {
        bail!(
            "{err}. ffmpeg stderr: {}",
            stderr.trim().if_empty_then("no stderr output")
        );
    }
    if !output.status.success() {
        bail!("ffmpeg failed while encoding video: {}", stderr.trim());
    }

    Ok(())
}

trait EmptyToFallback {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl EmptyToFallback for str {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{VideoCodec, VideoEncodeOptions, encode_raw_stream};

    fn ffmpeg_present() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[test]
    fn encode_raw_stream_with_ffmpeg() {
        if !ffmpeg_present() {
            eprintln!("ffmpeg not found on PATH; skipping");
            return;
        }

        let width = 8u32;
        let height = 8u32;
        let frame = vec![0u8; width as usize * height as usize * 3];
        let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("time");
        let mut output_path = std::env::temp_dir();
        output_path.push(format!("synthris-raw-{}.mp4", now.as_nanos()));

        let result = encode_raw_stream(
            &output_path,
            VideoEncodeOptions {
                fps: 12,
                codec: VideoCodec::Mp4H264,
            },
            width,
            height,
            |stdin| {
                for _ in 0..3 {
                    stdin.write_all(&frame)?;
                }
                Ok(())
            },
        );

        assert!(result.is_ok(), "ffmpeg raw encode failed: {result:?}");
        let size = fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
        assert!(size > 0, "encoded video file is empty");
        let _ = fs::remove_file(&output_path);
    }
}
