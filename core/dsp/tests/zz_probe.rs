// TEMPORARY review probe; not to be committed.
use dsp::{Snare, Voice, VoiceParams};

fn render(s: &mut Snare, n: usize) -> Vec<f32> {
    (0..n).map(|_| s.process()).collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn db(x: f64) -> f64 {
    10.0 * x.max(1e-30).log10()
}

/// Power spectrum (naive DFT with Hann), returns (freq, power) per bin.
fn spectrum(x: &[f32], sr: f32) -> Vec<(f64, f64)> {
    let n = x.len();
    let w: Vec<f64> = (0..n)
        .map(|i| {
            let h = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            f64::from(x[i]) * h
        })
        .collect();
    let mut out = Vec::new();
    for k in 0..n / 2 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        let step = std::f64::consts::TAU * k as f64 / n as f64;
        for (i, &v) in w.iter().enumerate() {
            let a = step * i as f64;
            re += v * a.cos();
            im -= v * a.sin();
        }
        out.push((k as f64 * f64::from(sr) / n as f64, re * re + im * im));
    }
    out
}

fn band(sp: &[(f64, f64)], lo: f64, hi: f64) -> f64 {
    sp.iter()
        .filter(|(f, _)| *f >= lo && *f < hi)
        .map(|(_, p)| p)
        .sum()
}

fn snare(sr: f32, f: impl FnOnce(&mut VoiceParams)) -> Snare {
    let mut s = Snare::new(sr);
    let mut p = VoiceParams::default();
    f(&mut p);
    s.apply_params(&p);
    s
}

#[test]
fn probe_extreme_rates() {
    for sr in [8_000.0f32, 11_025.0, 22_050.0, 44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        let mut worst = 0.0f32;
        let mut first = 0.0f32;
        for corner in 0..32u32 {
            let bit = |b: u32| if corner & (1 << b) != 0 { 1.0 } else { 0.0 };
            let mut s = snare(sr, |p| {
                p.tune = bit(0);
                p.decay = bit(1);
                p.tone = bit(2);
                p.snappy = bit(3);
                p.level = bit(4);
            });
            for v in [0.01, 0.1, 0.42, 0.7, 1.0] {
                s.trigger(v);
                let mut out = render(&mut s, (sr * 0.012) as usize);
                s.trigger(v);
                out.extend(render(&mut s, (sr * 1.0) as usize));
                for &x in &out {
                    assert!(x.is_finite() && x.abs() <= 1.0, "{sr} {corner} {v} {x}");
                }
                worst = worst.max(peak(&out));
                assert!(!s.is_active(), "{sr} {corner} {v} still active after 1 s");
            }
        }
        let mut s = Snare::new(sr);
        s.trigger(1.0);
        first = first.max(peak(&render(&mut s, sr as usize)));
        println!(
            "sr {sr}: default hit peak {:.2} dBFS, worst corner {:.2} dBFS",
            20.0 * first.log10(),
            20.0 * worst.log10()
        );
    }
}

#[test]
fn probe_aliasing_and_dc() {
    for (label, snappy, tone, tune) in [
        ("body bright", 0.0, 1.0, 1.0),
        ("full bright", 1.0, 1.0, 1.0),
        ("default", 0.5, 0.5, 0.5),
    ] {
        for sr in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut s = snare(sr, |p| {
                p.snappy = snappy;
                p.tone = tone;
                p.tune = tune;
            });
            s.trigger(1.0);
            let n = (sr * 0.1) as usize; // 100 ms
            let out = render(&mut s, n);
            let sp = spectrum(&out, sr);
            let total = band(&sp, 0.0, 1e9);
            let mean: f64 = out.iter().map(|&x| f64::from(x)).sum::<f64>() / n as f64;
            println!(
                "{label} @{sr}: <40Hz {:.1} | 40-1k {:.1} | 1-5k {:.1} | 5-10k {:.1} | 10-15k {:.1} | 15-18k {:.1} | 18-20k {:.1} | 20-22k {:.1} | 22k+ {:.1} (dB rel total) mean {:.2e}",
                db(band(&sp, 0.0, 40.0) / total),
                db(band(&sp, 40.0, 1_000.0) / total),
                db(band(&sp, 1_000.0, 5_000.0) / total),
                db(band(&sp, 5_000.0, 10_000.0) / total),
                db(band(&sp, 10_000.0, 15_000.0) / total),
                db(band(&sp, 15_000.0, 18_000.0) / total),
                db(band(&sp, 18_000.0, 20_000.0) / total),
                db(band(&sp, 20_000.0, 22_050.0) / total),
                db(band(&sp, 22_050.0, 1e9) / total),
                mean
            );
        }
    }
}

#[test]
fn probe_envelope() {
    let sr = 48_000.0;
    for (label, v) in [("accent", 1.0), ("plain", 0.7), ("ghost", 0.42)] {
        let mut s = Snare::new(sr);
        s.trigger(v);
        let out = render(&mut s, 24_000);
        let mut line = String::new();
        for w in out.chunks(240).take(40) {
            let rms = (w.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / w.len() as f64).sqrt();
            line += &format!("{:.0} ", 20.0 * rms.max(1e-9).log10());
        }
        println!("{label} RMS per 5 ms: {line}");
    }
    // body vs noise
    for snappy in [0.0, 0.5, 1.0] {
        let mut s = snare(sr, |p| p.snappy = snappy);
        s.trigger(1.0);
        let out = render(&mut s, 24_000);
        let mut line = String::new();
        for w in out.chunks(480).take(20) {
            let rms = (w.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / w.len() as f64).sqrt();
            line += &format!("{:.0} ", 20.0 * rms.max(1e-9).log10());
        }
        println!("snappy {snappy} RMS per 10 ms: {line} peak {:.2}", 20.0 * peak(&out).log10());
    }
}

#[test]
fn probe_body_vs_noise() {
    let sr = 48_000.0f32;
    for (decay, snappy, tone, v) in [
        (0.5, 0.5, 0.5, 1.0),
        (0.5, 0.5, 0.5, 0.7),
        (0.0, 0.5, 0.5, 1.0),
        (1.0, 0.5, 0.5, 1.0),
        (0.5, 1.0, 0.5, 1.0),
        (0.5, 0.5, 0.0, 1.0),
        (0.5, 0.5, 1.0, 1.0),
    ] {
        let mut b = snare(sr, |p| {
            p.decay = decay;
            p.snappy = 0.0;
            p.tone = tone;
        });
        let mut f = snare(sr, |p| {
            p.decay = decay;
            p.snappy = snappy;
            p.tone = tone;
        });
        b.trigger(v);
        f.trigger(v);
        let body = render(&mut b, 24_000);
        let full = render(&mut f, 24_000);
        let noise: Vec<f32> = full.iter().zip(&body).map(|(a, b)| a - b).collect();
        let env = |x: &[f32]| {
            x.chunks(480)
                .take(25)
                .map(|w| {
                    let r = (w.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / w.len() as f64)
                        .sqrt();
                    format!("{:.0}", 20.0 * r.max(1e-9).log10())
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let e = |x: &[f32], a: usize, bnd: usize| {
            x[a..bnd].iter().map(|&s| f64::from(s).powi(2)).sum::<f64>()
        };
        println!(
            "decay {decay} snappy {snappy} tone {tone} v {v}: noise/body energy 0-20ms {:.1} dB, 0-100ms {:.1} dB, 100-300ms {:.1} dB; peaks body {:.1} noise {:.1} full {:.1}",
            db(e(&noise, 0, 960) / e(&body, 0, 960)),
            db(e(&noise, 0, 4800) / e(&body, 0, 4800)),
            db(e(&noise, 4800, 14400) / e(&body, 4800, 14400)),
            20.0 * peak(&body).log10(),
            20.0 * peak(&noise).log10(),
            20.0 * peak(&full).log10(),
        );
        println!("   body  {}", env(&body));
        println!("   noise {}", env(&noise));
    }
}

#[test]
fn probe_flam_lf() {
    // Flam: grace (0.6) then main (1.0) after `gap`; compare LF energy of
    // the main hit's first 30 ms with a fresh main hit.
    let sr = 48_000.0f32;
    for snappy in [0.0, 0.5, 1.0] {
        for gap_ms in [8.0f32, 20.8, 40.0] {
            let gap = (gap_ms * 48.0) as usize;
            let mut worst_lf = 0.0f64;
            let mut fresh_lf = 0.0f64;
            let mut worst_off = 0.0f32;
            let mut s = snare(sr, |p| p.snappy = snappy);
            let mut fresh = snare(sr, |p| p.snappy = snappy);
            for _ in 0..50 {
                s.trigger(0.6);
                let _ = render(&mut s, gap);
                let last = s.process();
                worst_off = worst_off.max(last.abs());
                s.trigger(1.0);
                let main = render(&mut s, 1_440);
                let _ = render(&mut s, 24_000);
                fresh.trigger(1.0);
                let f = render(&mut fresh, 1_440);
                let _ = render(&mut fresh, 24_000);
                let lf = |x: &[f32]| {
                    // energy below ~100 Hz via 2x one-pole LP at 100 Hz
                    let a = (-std::f64::consts::TAU * 100.0 / 48_000.0).exp();
                    let (mut y1, mut y2, mut e) = (0.0f64, 0.0f64, 0.0f64);
                    for &v in x {
                        y1 = a * y1 + (1.0 - a) * f64::from(v);
                        y2 = a * y2 + (1.0 - a) * y1;
                        e += y2 * y2;
                    }
                    e
                };
                worst_lf = worst_lf.max(lf(&main));
                fresh_lf = fresh_lf.max(lf(&f));
            }
            println!(
                "snappy {snappy} gap {gap_ms} ms: worst LF {:.1} dB vs fresh LF {:.1} dB; worst offset {:.3}",
                db(worst_lf),
                db(fresh_lf),
                worst_off
            );
        }
    }
}

fn centroid(x: &[f32], sr: f32) -> f64 {
    let sp = spectrum(x, sr);
    let num: f64 = sp.iter().map(|(f, p)| f * p).sum();
    let den: f64 = sp.iter().map(|(_, p)| p).sum();
    num / den
}

#[test]
fn probe_monotonic() {
    let sr = 48_000.0f32;
    let mut line = String::from("tone centroid: ");
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        let mut s = snare(sr, |p| p.tone = t);
        s.trigger(1.0);
        let out = render(&mut s, 2_400);
        line += &format!("{:.0} ", centroid(&out, sr));
    }
    println!("{line}");
    let mut line = String::from("velocity peak dB / centroid: ");
    for i in 1..=10 {
        let v = i as f32 / 10.0;
        let mut s = Snare::new(sr);
        s.trigger(v);
        let out = render(&mut s, 2_400);
        line += &format!("{:.1}/{:.0} ", 20.0 * peak(&out).log10(), centroid(&out, sr));
    }
    println!("{line}");
    let mut line = String::from("snappy 2-10k energy dB: ");
    for i in 0..=10 {
        let v = i as f32 / 10.0;
        let mut s = snare(sr, |p| p.snappy = v);
        s.trigger(1.0);
        let out = render(&mut s, 4_800);
        let sp = spectrum(&out[..2_400], sr);
        line += &format!("{:.1} ", db(band(&sp, 2_000.0, 10_000.0)));
    }
    println!("{line}");
    let mut line = String::from("decay energy 100-400ms dB: ");
    for i in 0..=10 {
        let v = i as f32 / 10.0;
        let mut s = snare(sr, |p| p.decay = v);
        s.trigger(1.0);
        let out = render(&mut s, 19_200);
        let e: f64 = out[4_800..].iter().map(|&x| f64::from(x).powi(2)).sum();
        line += &format!("{:.1} ", db(e));
    }
    println!("{line}");
}

#[test]
fn probe_garbage() {
    for sr in [0.0f32, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0, 100.0, 1e9, f32::MAX, f32::MIN_POSITIVE] {
        for pv in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 5.0, 0.0, 1.0] {
            let mut s = Snare::new(sr);
            s.apply_params(&VoiceParams {
                tune: pv,
                decay: pv,
                tone: pv,
                snappy: pv,
                level: pv,
            });
            for v in [f32::NAN, f32::INFINITY, -1.0, 1.0, 1e-30, f32::MIN_POSITIVE, 2.0] {
                s.trigger(v);
                for _ in 0..2_000 {
                    let x = s.process();
                    assert!(x.is_finite() && x.abs() <= 1.0, "sr {sr} pv {pv} v {v}: {x}");
                }
            }
            let mut n = 0;
            while s.is_active() && n < 50_000_000 {
                let x = s.process();
                assert!(x.is_finite() && x.abs() <= 1.0);
                n += 1;
            }
            if s.is_active() {
                println!("sr {sr} pv {pv}: still active after 50M samples");
            }
        }
    }
}

#[test]
fn probe_retrigger() {
    let sr = 48_000.0f32;
    // Roll: 32nd notes at 174 bpm, and very fast retriggers.
    for (label, period) in [("1ms", 48usize), ("2ms", 96), ("5ms", 240), ("10ms", 480), ("32nd@174", 517)] {
        for snappy in [0.0, 0.5] {
            let mut s = snare(sr, |p| p.snappy = snappy);
            let mut out = Vec::new();
            for _ in 0..40 {
                s.trigger(1.0);
                out.extend(render(&mut s, period));
            }
            let steps: Vec<f32> = out.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
            let ms = steps.iter().cloned().fold(0.0f32, f32::max);
            let mut fresh = snare(sr, |p| p.snappy = snappy);
            fresh.trigger(1.0);
            let f = render(&mut fresh, 4_800);
            let fs = f.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
            let mean: f64 = out.iter().map(|&x| f64::from(x)).sum::<f64>() / out.len() as f64;
            println!(
                "roll {label} snappy {snappy}: peak {:.2} dBFS, max step {ms:.4} vs fresh {fs:.4}, mean {mean:.4}",
                20.0 * peak(&out).log10()
            );
        }
    }
}
