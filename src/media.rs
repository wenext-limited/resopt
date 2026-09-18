//! Audio and video inspection through the optional `ffprobe` tool.
//!
//! resopt does not transcode audio or video: every useful reduction is lossy
//! and depends on how the app plays the file. It reports codec, duration and
//! bitrate so oversized media is easy to spot, and says so when ffprobe is
//! missing instead of pretending the files were analyzed.
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::OnceLock,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub container: Option<String>,
    pub duration_seconds: Option<f64>,
    pub bit_rate: Option<u64>,
    /// `codec_type/codec_name` per stream, e.g. `video/h264`.
    pub streams: Vec<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

pub(crate) fn ffprobe_available() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        Command::new("ffprobe")
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Parse `ffprobe -of json -show_format -show_streams` output.
pub(crate) fn parse(json: &[u8]) -> Option<MediaInfo> {
    let value: serde_json::Value = serde_json::from_slice(json).ok()?;
    let format = value.get("format")?;
    let number = |v: &serde_json::Value| v.as_str().and_then(|s| s.parse::<f64>().ok());
    let streams = value.get("streams")?.as_array()?;
    let video = streams.iter().find(|s| s["codec_type"] == "video");
    Some(MediaInfo {
        container: format["format_name"].as_str().map(str::to_string),
        duration_seconds: number(&format["duration"]).filter(|d| d.is_finite() && *d >= 0.0),
        bit_rate: number(&format["bit_rate"]).map(|b| b as u64),
        streams: streams
            .iter()
            .map(|s| {
                format!(
                    "{}/{}",
                    s["codec_type"].as_str().unwrap_or("unknown"),
                    s["codec_name"].as_str().unwrap_or("unknown")
                )
            })
            .collect(),
        width: video.and_then(|s| s["width"].as_u64()).map(|v| v as u32),
        height: video.and_then(|s| s["height"].as_u64()).map(|v| v as u32),
    })
}

pub(crate) fn probe(path: &Path) -> Option<MediaInfo> {
    if !ffprobe_available() {
        return None;
    }
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-of",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| parse(&output.stdout))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffprobe_json_and_rejects_garbage() {
        let info = parse(br#"{"streams":[{"codec_type":"video","codec_name":"h264","width":720,"height":1280},{"codec_type":"audio","codec_name":"aac"}],"format":{"format_name":"mov,mp4","duration":"3.500000","bit_rate":"1250000"}}"#).unwrap();
        assert_eq!(info.streams, ["video/h264", "audio/aac"]);
        assert_eq!((info.width, info.height), (Some(720), Some(1280)));
        assert_eq!(info.duration_seconds, Some(3.5));
        assert_eq!(info.bit_rate, Some(1_250_000));
        assert_eq!(parse(b"not json"), None);
        assert_eq!(parse(br#"{"format":{}}"#), None);
        let sparse = parse(br#"{"streams":[],"format":{"duration":"nan"}}"#).unwrap();
        assert_eq!(sparse.duration_seconds, None);
    }
}
