//! The [`Voice`] contract, checked for every voice in the kit.

use dsp::{Clap, ClosedHat, Cowbell, Kick, OpenHat, Rim, Snare, Tom, TomRange, Voice, VoiceParams};

const SR: f32 = 48_000.0;

/// Builds a fresh voice.
type Make<'a> = &'a dyn Fn() -> Box<dyn VoiceClone>;

/// Runs `check` against a fresh instance of every voice type.
fn for_every_voice(check: &dyn Fn(&str, Make)) {
    check("kick", &|| Box::new(Kick::new(SR)));
    check("snare", &|| Box::new(Snare::new(SR)));
    check("low tom", &|| Box::new(Tom::new(SR, TomRange::Low)));
    check("mid tom", &|| Box::new(Tom::new(SR, TomRange::Mid)));
    check("high tom", &|| Box::new(Tom::new(SR, TomRange::High)));
    check("rim", &|| Box::new(Rim::new(SR)));
    check("clap", &|| Box::new(Clap::new(SR)));
    check("closed hat", &|| Box::new(ClosedHat::new(SR)));
    check("open hat", &|| Box::new(OpenHat::new(SR)));
    check("cowbell", &|| Box::new(Cowbell::new(SR)));
}

/// A voice that can be duplicated mid-hit, to compare two futures.
trait VoiceClone: Voice {
    fn duplicate(&self) -> Box<dyn VoiceClone>;
}

impl<V: Voice + Clone + 'static> VoiceClone for V {
    fn duplicate(&self) -> Box<dyn VoiceClone> {
        Box::new(self.clone())
    }
}

fn render(voice: &mut dyn VoiceClone, n: usize) -> Vec<f32> {
    (0..n).map(|_| voice.process()).collect()
}

fn with_level(level: f32) -> VoiceParams {
    VoiceParams {
        level,
        ..VoiceParams::default()
    }
}

#[test]
fn a_silent_hit_neither_starts_nor_cuts_a_sound() {
    for_every_voice(&|name, make| {
        let mut idle = make();
        idle.trigger(0.0);
        idle.trigger(f32::NAN);
        assert!(!idle.is_active(), "{name}: velocity 0 started a hit");
        assert!(render(&mut *idle, 100).iter().all(|&s| s == 0.0), "{name}");

        let mut a = make();
        a.trigger(1.0);
        render(&mut *a, 480);
        let mut b = a.duplicate();
        b.trigger(0.0);
        b.trigger(f32::NAN);
        b.trigger(-1.0);
        let (ra, rb) = (render(&mut *a, 4_800), render(&mut *b, 4_800));
        assert!(
            ra.iter().zip(&rb).all(|(x, y)| x.to_bits() == y.to_bits()),
            "{name}: a velocity-0 hit changed a sounding voice"
        );
    });
}

#[test]
fn level_changes_on_a_sounding_voice_are_smoothed() {
    for_every_voice(&|name, make| {
        let mut params = VoiceParams {
            decay: 1.0,
            ..VoiceParams::default()
        };
        let mut a = make();
        a.apply_params(&params);
        a.trigger(1.0);
        // Into the body of the hit, where every voice is still loud.
        render(&mut *a, 240);
        let mut b = a.duplicate();
        params.level = 0.0;
        b.apply_params(&params);
        let ra = render(&mut *a, 4_800);
        let rb = render(&mut *b, 4_800);
        // The first samples after the change barely move...
        for i in 0..4 {
            assert!(
                rb[i].abs() >= 0.9 * ra[i].abs(),
                "{name}: sample {i} after a level change jumped ({} vs {})",
                rb[i],
                ra[i]
            );
        }
        // ...and within 50 ms the voice has followed the fader down.
        let peak = ra.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let late = rb[2_400..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(late <= 1e-3 * peak.max(1e-6), "{name}: {late} vs {peak}");
    });
}

#[test]
fn a_level_set_while_idle_applies_from_the_first_sample() {
    for_every_voice(&|name, make| {
        let mut loud = make();
        let mut quiet = make();
        quiet.apply_params(&with_level(0.0));
        loud.trigger(1.0);
        quiet.trigger(1.0);
        assert!(
            render(&mut *loud, 480).iter().any(|&s| s != 0.0),
            "{name}: silent hit"
        );
        assert!(
            render(&mut *quiet, 480).iter().all(|&s| s == 0.0),
            "{name}: level 0 set while idle was smoothed into the hit"
        );
    });
}

#[test]
fn set_sample_rate_restores_the_fresh_state() {
    for_every_voice(&|name, make| {
        let mut fresh = make();
        let mut used = make();
        used.trigger(1.0);
        render(&mut *used, 2_000);
        used.trigger(0.8);
        render(&mut *used, 500);
        used.set_sample_rate(SR);
        assert!(!used.is_active(), "{name}: still sounding");
        fresh.trigger(1.0);
        used.trigger(1.0);
        let (rf, ru) = (render(&mut *fresh, 9_600), render(&mut *used, 9_600));
        assert!(
            rf.iter().zip(&ru).all(|(x, y)| x.to_bits() == y.to_bits()),
            "{name}: a render after set_sample_rate depends on what played before"
        );
    });
}

#[test]
fn hostile_inputs_stay_finite_and_bounded() {
    for_every_voice(&|name, make| {
        let mut v = make();
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0, 5.0] {
            v.apply_params(&VoiceParams {
                tune: bad,
                decay: bad,
                tone: bad,
                snappy: bad,
                level: bad,
            });
            v.trigger(bad);
            v.trigger(1.0);
            for s in render(&mut *v, 2_000) {
                assert!(s.is_finite() && s.abs() <= 1.0, "{name}: {s} after {bad}");
            }
        }
    });
}
