use super::*;
use std::io::Write;
#[test]
fn gzip_requests_are_bounded_after_decompression() {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&vec![b'a'; 256 * 1024]).unwrap();
    let encoded = encoder.finish().unwrap();
    assert!(matches!(
        decode_gzip(&encoded, 128 * 1024),
        Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, _))
    ));
    assert!(decode_gzip(b"bad gzip", 128 * 1024).is_err());
    assert!(matches!(decode_gzip(&encoded, 256 * 1024), Ok(body) if body.len() == 256 * 1024));
}
