//! SVGA 2.x container: one zlib stream holding a protobuf `MovieEntity`.
use super::Refusal;
use std::io::Read;

/// Inflate the whole file, refusing anything that is not exactly one zlib
/// stream or that expands beyond `limit` bytes.
pub(super) fn inflate(bytes: &[u8], limit: usize) -> Result<Vec<u8>, Refusal> {
    if bytes.starts_with(b"PK") {
        return Err(Refusal::Unsupported("svga_1x_zip_not_supported"));
    }
    if !is_zlib_header(bytes) {
        return Err(Refusal::Malformed("svga_not_zlib"));
    }
    // `&[u8]` is already buffered, so the decoder never reads past the stream
    // end and `total_in` is the exact stream length.
    let mut decoder = flate2::bufread::ZlibDecoder::new(bytes);
    let mut inflated = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    decoder
        .by_ref()
        .take(cap)
        .read_to_end(&mut inflated)
        .map_err(|_| Refusal::Malformed("svga_corrupt_zlib_stream"))?;
    if inflated.len() > limit {
        return Err(Refusal::Unsupported("svga_inflated_size_exceeds_limit"));
    }
    if decoder.total_in() != bytes.len() as u64 {
        return Err(Refusal::Malformed("svga_trailing_bytes"));
    }
    Ok(inflated)
}

fn is_zlib_header(bytes: &[u8]) -> bool {
    match bytes {
        [cmf, flags, ..] => {
            cmf & 0x0f == 8 && (u16::from(*cmf) << 8 | u16::from(*flags)).is_multiple_of(31)
        }
        _ => false,
    }
}

/// Maximum-effort zlib stream.
pub(super) fn deflate(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut compressor = libdeflater::Compressor::new(libdeflater::CompressionLvl::best());
    let mut compressed = vec![0; compressor.zlib_compress_bound(bytes.len())];
    let length = compressor
        .zlib_compress(bytes, &mut compressed)
        .map_err(|error| anyhow::anyhow!("svga_deflate_failed: {error}"))?;
    compressed.truncate(length);
    Ok(compressed)
}
