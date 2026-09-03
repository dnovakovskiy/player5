//! `cargo run -p render -- pattern.json out.wav [--fingerprint]`
//!
//! Renders a pattern file offline. With `--fingerprint`, prints the PCM
//! hash and fingerprint JSON that the golden-master tests compare against.

use std::path::PathBuf;
use std::process::ExitCode;

use engine::PatternSpec;
use render::analysis::{hash_pcm16, Fingerprint};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fingerprint = args.iter().any(|a| a == "--fingerprint");
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let (Some(input), Some(output)) = (positional.first(), positional.get(1)) else {
        eprintln!("usage: render <pattern.json> <out.wav> [--fingerprint]");
        return ExitCode::from(2);
    };
    let input = PathBuf::from(input);
    let output = PathBuf::from(output);

    let json = match std::fs::read_to_string(&input) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("cannot read {}: {e}", input.display());
            return ExitCode::from(1);
        }
    };
    let spec = match PatternSpec::from_json(&json) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {e}", input.display());
            return ExitCode::from(1);
        }
    };
    let audio = match spec.render() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("render failed: {e}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = render::write_wav(&output, spec.render.sample_rate, &audio) {
        eprintln!("cannot write {}: {e}", output.display());
        return ExitCode::from(1);
    }
    let seconds = audio.len() as f64 / f64::from(spec.render.sample_rate);
    eprintln!(
        "wrote {} ({} frames, {seconds:.2} s at {} Hz)",
        output.display(),
        audio.len(),
        spec.render.sample_rate
    );
    if fingerprint {
        let fp = Fingerprint::of(&audio, spec.render.sample_rate);
        println!(
            "{}",
            serde_json::json!({
                "hash_pcm16_fnv1a64": format!("{:#018x}", hash_pcm16(&audio)),
                "fingerprint": fp,
            })
        );
    }
    ExitCode::SUCCESS
}
