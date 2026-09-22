//! VAP stores an RGB region and an alpha region inside an ordinary MP4 video.
//! Only bounded top-level `vapc` metadata is parsed; encoded video is unchanged.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VapInfo {
    pub width: u32,
    pub height: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub frames: u32,
    pub fps: f64,
    pub rgb_frame: [u32; 4],
    pub alpha_frame: [u32; 4],
    pub dynamic_sources: usize,
}

#[derive(Deserialize)]
struct Config {
    info: Info,
    #[serde(default)]
    src: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct Info {
    v: u32,
    w: u32,
    h: u32,
    f: u32,
    fps: f64,
    #[serde(rename = "videoW")]
    video_w: u32,
    #[serde(rename = "videoH")]
    video_h: u32,
    #[serde(rename = "rgbFrame")]
    rgb: [u32; 4],
    #[serde(rename = "aFrame")]
    alpha: [u32; 4],
}

pub(crate) fn inspect(bytes: &[u8]) -> Result<Option<VapInfo>> {
    if bytes.get(4..8) != Some(b"ftyp") {
        return Ok(None);
    }
    let mut at = 0_usize;
    let mut boxes = 0;
    let mut found = None;
    while at < bytes.len() {
        boxes += 1;
        ensure!(boxes <= 10_000, "mp4_box_count_limit");
        let header = bytes.get(at..at + 8).context("mp4_truncated_box")?;
        let length = u32::from_be_bytes(header[..4].try_into()?) as u64;
        let (length, header_len) = match length {
            0 => ((bytes.len() - at) as u64, 8),
            1 => (
                u64::from_be_bytes(
                    bytes
                        .get(at + 8..at + 16)
                        .context("mp4_truncated_large_box")?
                        .try_into()?,
                ),
                16,
            ),
            _ => (length, 8),
        };
        ensure!(
            length >= header_len as u64 && length <= (bytes.len() - at) as u64,
            "mp4_invalid_box_size"
        );
        let end = at + length as usize;
        if &header[4..] == b"vapc" {
            ensure!(found.is_none(), "vap_duplicate_config");
            ensure!(length <= 1024 * 1024, "vap_config_size_limit");
            let config: Config = serde_json::from_slice(&bytes[at + header_len..end])
                .context("vap_invalid_config")?;
            let i = config.info;
            ensure!(
                i.v == 2
                    && i.w > 0
                    && i.h > 0
                    && i.video_w > 0
                    && i.video_h > 0
                    && i.video_w <= 8192
                    && i.video_h <= 8192
                    && u64::from(i.w) * u64::from(i.h) <= 16_777_216
                    && i.f > 0
                    && i.f <= 1_000_000
                    && i.fps.is_finite()
                    && (1.0..=120.0).contains(&i.fps),
                "vap_unsupported_canvas_or_timing"
            );
            for rect in [i.rgb, i.alpha] {
                ensure!(
                    rect[2] > 0
                        && rect[3] > 0
                        && rect[0].checked_add(rect[2]).is_some_and(|x| x <= i.video_w)
                        && rect[1].checked_add(rect[3]).is_some_and(|y| y <= i.video_h),
                    "vap_region_out_of_bounds"
                );
            }
            found = Some(VapInfo {
                width: i.w,
                height: i.h,
                video_width: i.video_w,
                video_height: i.video_h,
                frames: i.f,
                fps: i.fps,
                rgb_frame: i.rgb,
                alpha_frame: i.alpha,
                dynamic_sources: config.src.len(),
            });
        }
        at = end;
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mp4(config: serde_json::Value) -> Vec<u8> {
        let mut bytes = vec![0, 0, 0, 16];
        bytes.extend(b"ftypisom0000");
        let json = serde_json::to_vec(&config).unwrap();
        bytes.extend(((json.len() + 8) as u32).to_be_bytes());
        bytes.extend(b"vapc");
        bytes.extend(json);
        bytes
    }
    fn config() -> serde_json::Value {
        serde_json::json!({"info":{"v":2,"w":100,"h":80,"f":30,"fps":25,"videoW":150,"videoH":80,"rgbFrame":[0,0,100,80],"aFrame":[100,0,50,40]},"src":[{"srcId":"avatar"}]})
    }
    #[test]
    fn detects_vap_and_preserves_regions_and_dynamic_content_count() {
        let info = inspect(&mp4(config())).unwrap().unwrap();
        assert_eq!((info.width, info.height, info.frames), (100, 80, 30));
        assert_eq!(info.alpha_frame, [100, 0, 50, 40]);
        assert_eq!(info.dynamic_sources, 1);
        assert!(inspect(b"ordinary data").unwrap().is_none());
    }
    #[test]
    fn rejects_invalid_regions_timing_and_container_sizes() {
        let mut invalid = config();
        invalid["info"]["aFrame"] = serde_json::json!([149, 0, 50, 40]);
        assert!(inspect(&mp4(invalid)).is_err());
        let mut invalid = config();
        invalid["info"]["fps"] = serde_json::json!(0);
        assert!(inspect(&mp4(invalid)).is_err());
        let mut bytes = mp4(config());
        bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(inspect(&bytes).is_err());
        let mut bytes = mp4(config());
        bytes.extend_from_slice(&mp4(config())[16..]);
        assert!(inspect(&bytes).is_err());
    }
}
