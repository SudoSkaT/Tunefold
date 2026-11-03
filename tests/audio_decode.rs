//! Reproducible AAC/ADTS regression tests.
//!
//! The fixtures are tiny (≈1.2 s) and cover the two layouts that used to fail:
//! plain ADTS (`channel_configuration != 0`) and ADTS/MP4 with a Program
//! Config Element (`channel_configuration = 0`).

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SignalSpec;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

struct Decoded {
    spec: SignalSpec,
    frames: u64,
    packets: u64,
}

fn decode(path: &Path) -> Decoded {
    let file = File::open(path).unwrap();
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    hint.with_extension(path.extension().and_then(|e| e.to_str()).unwrap_or("aac"));

    let mut probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .unwrap();

    let track = probed
        .format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .unwrap()
        .clone();

    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .unwrap();

    let mut frames = 0u64;
    let mut packets = 0u64;
    let mut spec = None;

    loop {
        let packet = match probed.format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(_)) => break,
            Err(e) => panic!("next_packet: {e}"),
        };

        match decoder.decode(&packet) {
            Ok(audio) => {
                frames += audio.frames() as u64;
                packets += 1;
                spec = Some(audio.spec().to_owned());
            }
            Err(Error::IoError(_)) => break,
            Err(e) => panic!("decode {} packet {}: {e}", path.display(), packets),
        }
    }

    Decoded {
        spec: spec.expect("at least one decoded buffer"),
        frames,
        packets,
    }
}

fn assert_stereo_44k(path: &str) {
    let decoded = decode(&Path::new(env!("CARGO_MANIFEST_DIR")).join(path));
    assert_eq!(decoded.spec.rate, 44_100, "{path}: sample rate");
    assert_eq!(decoded.spec.channels.count(), 2, "{path}: channels");
    assert!(decoded.frames >= 44_100, "{path}: decoded too few frames");
    assert!(decoded.packets > 1, "{path}: expected multiple packets");
}

#[test]
fn adts_plain_decodes() {
    assert_stereo_44k("tests/fixtures/audio/plain.aac");
}

#[test]
fn adts_with_program_config_decodes() {
    assert_stereo_44k("tests/fixtures/audio/pce.aac");
}

#[test]
fn mp4_plain_decodes() {
    assert_stereo_44k("tests/fixtures/audio/plain.m4a");
}

#[test]
fn mp4_with_program_config_decodes() {
    assert_stereo_44k("tests/fixtures/audio/pce.m4a");
}

fn source_frames() -> Vec<Vec<u8>> {
    let data =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/plain.aac"))
            .unwrap();
    let mut frames = Vec::new();
    let mut pos = 0;
    while pos + 7 <= data.len() {
        let header_size = if data[pos + 1] & 1 == 1 { 7 } else { 9 };
        let frame_len = (((data[pos + 3] & 0x03) as usize) << 11)
            | ((data[pos + 4] as usize) << 3)
            | ((data[pos + 5] as usize) >> 5);
        assert!(frame_len >= header_size && pos + frame_len <= data.len());
        frames.push(data[pos..pos + frame_len].to_vec());
        pos += frame_len;
    }
    assert!(!frames.is_empty());
    frames
}

fn adts_header(
    source: &[u8],
    payload_len: usize,
    num_blocks: u8,
    crc: bool,
    mpeg2: bool,
) -> Vec<u8> {
    let header_size = if crc { 9 } else { 7 };
    let total = header_size + payload_len;
    assert!(total < (1 << 13));
    let mut header = vec![0u8; header_size];
    header[0] = 0xff;
    header[1] = 0xf0 | if mpeg2 { 0x08 } else { 0 } | if crc { 0 } else { 1 };
    header[2] = source[2];
    header[3] = (source[3] & 0xf0) | ((total >> 11) as u8 & 0x03);
    header[4] = (total >> 3) as u8;
    header[5] = (((total & 0x07) as u8) << 5) | (source[5] & 0x1f);
    header[6] = (source[6] & 0xfc) | (num_blocks - 1);
    header
}

fn decode_bytes(data: &[u8]) -> Decoded {
    use std::io::Write;
    let mut file = tempfile::Builder::new().suffix(".aac").tempfile().unwrap();
    file.write_all(data).unwrap();
    file.flush().unwrap();
    decode(file.path())
}

#[test]
fn adts_multi_block_packet_realigns() {
    let frames = source_frames();
    let first = &frames[0];
    let second = &frames[1];
    let mut payload = first[7..].to_vec();
    payload.extend_from_slice(&second[7..]);
    let mut stream = adts_header(first, payload.len(), 2, false, false);
    stream.extend_from_slice(&payload);

    let decoded = decode_bytes(&stream);
    assert_eq!(decoded.spec.channels.count(), 2);
    assert_eq!(decoded.spec.rate, 44_100);
    assert_eq!(
        decoded.frames, 2048,
        "one ADTS packet with two raw_data_blocks"
    );
}

#[test]
fn adts_crc_and_mpeg2_headers_decode() {
    let frames = source_frames();
    let first = &frames[0];

    for (crc, mpeg2) in [(false, true), (true, false), (true, true)] {
        let payload = first[7..].to_vec();
        let mut stream = adts_header(first, payload.len(), 1, crc, mpeg2);
        stream.extend_from_slice(&payload);

        let decoded = decode_bytes(&stream);
        assert_eq!(decoded.spec.channels.count(), 2);
        assert_eq!(decoded.frames, 1024);
    }
}
