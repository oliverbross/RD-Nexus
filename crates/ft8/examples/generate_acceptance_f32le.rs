// SPDX-License-Identifier: GPL-3.0-only
//! Generate a deterministic, genuine FT8 receive fixture for packaged acceptance.

use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    let mut args = env::args().skip(1);
    let output = PathBuf::from(
        args.next()
            .expect("usage: generate_acceptance_f32le OUTPUT [MESSAGE]"),
    );
    let message = args.next().unwrap_or_else(|| "N0TST K1ABC -12".to_string());
    assert!(args.next().is_none(), "too many arguments");

    let tones = ft8::encode(&message);
    assert!(!tones.is_empty(), "message is not FT8 encodable");
    let wave = ft8::gen_wave(&tones, ft8::SAMPLE_RATE, 1_500.0);
    let mut frame = vec![0.0_f32; ft8::NMAX];
    let offset = (0.5 * ft8::SAMPLE_RATE) as usize;
    frame[offset..offset + wave.len()].copy_from_slice(&wave);

    let check: Vec<i16> = frame
        .iter()
        .map(|sample| (sample * 12_000.0) as i16)
        .collect();
    let decoded = ft8::decode_frame(&check, 200, 2_900, 3, "N0TST", "", 0, 0, true, false);
    assert!(
        decoded.iter().any(|entry| entry.message == message),
        "generated fixture did not decode: {decoded:?}"
    );

    // The file backend begins at launch. Lead with silence to the next UTC FT8
    // boundary, then repeat four complete slots so normal UI startup has time
    // to become visible without replay or a synthetic decoder callback.
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_millis() as u64;
    let lead_ms = 15_000 - (now_ms % 15_000);
    let lead_samples = (lead_ms * ft8::SAMPLE_RATE as u64 / 1_000) as usize;
    let mut samples = vec![0.0_f32; lead_samples];
    for _ in 0..4 {
        samples.extend_from_slice(&frame);
    }
    let mut bytes = Vec::with_capacity(samples.len() * size_of::<f32>());
    for sample in samples {
        bytes.extend_from_slice(&(sample * 0.5).to_le_bytes());
    }
    fs::write(&output, bytes).expect("write fixture");
    println!(
        "wrote {} as {} ({} ms to next UTC slot)",
        output.display(),
        message,
        lead_ms
    );
}
