//! The supersaw bank against floating point: Szabo's curves, the aliasing
//! `PolyBLEP` removes, the high-pass and the parts (journal/D-037).

use std::f64::consts::TAU;

use oc_core::audio::supersaw::{FULL, Part, Supersaw, detune_curve, mix_gains};
use oc_core::audio::wave::Waveform;
use oc_core::audio::{Channel, Params, Tone, Voice};

/// Szabo's detune curve, highest power first.
const DETUNE_POLYNOMIAL: [f64; 12] = [
    10_028.731_2,
    -50_818.865_2,
    111_363.480_8,
    -138_150.676_1,
    106_649.667_9,
    -53_046.964_2,
    17_019.951_8,
    -3_425.083_6,
    404.270_3,
    -24.187_8,
    0.671_7,
    0.003_0,
];

fn detune_exact(x: f64) -> f64 {
    DETUNE_POLYNOMIAL
        .iter()
        .fold(0.0, |value, coefficient| value * x + coefficient)
}

#[test]
fn the_detune_table_follows_szabos_polynomial() {
    let mut worst = 0.0f64;
    for detune in 0..=FULL {
        let exact = detune_exact(f64::from(detune) / f64::from(FULL));
        #[allow(clippy::cast_precision_loss)]
        // A 16.16 value below two.
        let got = detune_curve(detune) as f64 / 65_536.0;
        worst = worst.max((got - exact).abs());
    }
    // The steep end of the curve, between its last two points, is the worst.
    assert!(worst < 0.02, "worst error {worst}");
}

#[test]
fn the_mix_laws_follow_szabos() {
    for mix in (0..=FULL).step_by(10) {
        let x = f64::from(mix) / f64::from(FULL);
        let centre = -0.553_66 * x + 0.997_85;
        let side = -0.737_64 * x * x + 1.284_1 * x + 0.044_372;
        let (got_centre, got_side) = mix_gains(mix);
        #[allow(clippy::cast_precision_loss)]
        // 1.15 values.
        let (got_centre, got_side) = (got_centre as f64 / 32_768.0, got_side as f64 / 32_768.0);
        assert!((got_centre - centre).abs() < 1e-3, "{mix}: {got_centre}");
        assert!((got_side - side).abs() < 1e-3, "{mix}: {got_side}");
    }
}

/// A voice at 48 kHz playing `supersaw` on A (mono), B and C (left and
/// right) and D (the sub-octave).
fn bank_voice(supersaw: Supersaw) -> Voice {
    let mut voice = Voice::new(48_000);
    let mut params = Params::REST;
    params.channels = [
        Channel::Supersaw(Part::Mono),
        Channel::Supersaw(Part::Left),
        Channel::Supersaw(Part::Right),
        Channel::Supersaw(Part::Sub),
    ];
    params.supersaw = supersaw;
    voice.set_params(&params);
    voice
}

fn supersaw(frequency: u32, detune: u16, mix: u16) -> Supersaw {
    Supersaw {
        frequency,
        detune,
        mix,
        amplitude: 1_000,
    }
}

/// The power at `frequency` of `samples` at 48 kHz (Goertzel).
fn power(samples: &[f64], frequency: f64) -> f64 {
    let coefficient = 2.0 * (TAU * frequency / 48_000.0).cos();
    let (mut s1, mut s2) = (0.0, 0.0);
    for sample in samples {
        let s0 = sample + coefficient * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    s1 * s1 + s2 * s2 - coefficient * s1 * s2
}

/// Alias power under 10 kHz over harmonic power, in dB, of 0.1 s of a
/// sawtooth at `fundamental` hertz, through a Hann window.
fn aliasing_db(samples: &[i32], fundamental: u32) -> f64 {
    let length = samples.len();
    #[allow(clippy::cast_precision_loss)]
    // A few thousand samples.
    let windowed: Vec<f64> = samples
        .iter()
        .enumerate()
        .map(|(index, &sample)| {
            let window = 0.5 - 0.5 * (TAU * index as f64 / length as f64).cos();
            f64::from(sample) * window
        })
        .collect();
    let rate = 48_000;
    let harmonic_power: f64 = (1..400)
        .map(|n| n * fundamental)
        .take_while(|&frequency| frequency < rate / 2)
        .map(|frequency| power(&windowed, f64::from(frequency)))
        .sum();
    let mut aliases: Vec<u32> = (1..400)
        .map(|n| n * fundamental)
        .filter(|&frequency| frequency > rate / 2)
        .map(|frequency| {
            let folded = frequency % rate;
            folded.min(rate - folded)
        })
        .filter(|&alias| (20..10_000).contains(&alias))
        .collect();
    aliases.sort_unstable();
    aliases.dedup();
    let alias_power: f64 = aliases
        .iter()
        .map(|&alias| power(&windowed, f64::from(alias)))
        .sum();
    10.0 * (alias_power / harmonic_power).log10()
}

#[test]
fn polyblep_removes_most_of_the_aliasing_of_a_naive_saw() {
    // The sub-octave is a single corrected sawtooth: 1 970 Hz under a
    // 3 940 Hz fundamental. Its aliases land between its harmonics.
    let fundamental = 1_970;
    let mut corrected = bank_voice(supersaw(2 * fundamental * 1_000, 0, 0));
    let mut naive = Voice::new(48_000);
    let mut params = Params::REST;
    params.channels[0] = Channel::Tone(Tone {
        wave: Waveform::Saw,
        frequency: fundamental * 1_000,
        amplitude: 1_000,
    });
    naive.set_params(&params);
    let corrected: Vec<i32> = (0..4_800).map(|_| corrected.next_sample()[3]).collect();
    let naive: Vec<i32> = (0..4_800).map(|_| naive.next_sample()[0]).collect();
    let (naive, corrected) = (
        aliasing_db(&naive, fundamental),
        aliasing_db(&corrected, fundamental),
    );
    assert!(
        corrected < naive - 25.0,
        "naive {naive:.1} dB, PolyBLEP {corrected:.1} dB"
    );
}

#[test]
fn the_high_pass_leaves_no_offset() {
    let mut voice = bank_voice(supersaw(110_000, 500, 500));
    let samples: Vec<i32> = (0..48_000).map(|_| voice.next_sample()[0]).collect();
    let tail = &samples[4_800..];
    #[allow(clippy::cast_precision_loss)]
    // Fewer than 2^24 samples.
    let mean = tail.iter().map(|&sample| f64::from(sample)).sum::<f64>() / tail.len() as f64;
    assert!(mean.abs() < 5.0, "mean {mean} mV");
}

/// Root mean square of `part` over a second at 110 Hz.
fn rms(detune: u16, mix: u16, part: usize) -> f64 {
    let mut voice = bank_voice(supersaw(110_000, detune, mix));
    let sum: f64 = (0..48_000)
        .map(|_| f64::from(voice.next_sample()[part]).powi(2))
        .sum();
    (sum / 48_000.0).sqrt()
}

#[test]
fn the_mix_changes_the_balance_more_than_the_loudness() {
    for part in [0, 1, 2] {
        let (dry, wet) = (rms(500, 0, part), rms(500, FULL, part));
        let ratio = 20.0 * (wet / dry).log10();
        assert!(ratio.abs() < 3.0, "part {part}: {ratio:.1} dB");
    }
}

#[test]
fn left_and_right_differ_only_by_their_side_voices() {
    let mut voice = bank_voice(supersaw(220_000, FULL, FULL));
    let mut different = 0;
    for _ in 0..4_800 {
        let [_, left, right, _] = voice.next_sample();
        if (left - right).abs() > 50 {
            different += 1;
        }
    }
    assert!(different > 1_000, "{different} samples apart");
}

/// Jumps of `part` over a second: rises of more than its peak level over
/// two samples, `PolyBLEP` spreading each over two (the sawtooths fall and
/// jump up); the side voices, far quieter, never rise that much.
fn jumps(supersaw: Supersaw, part: usize) -> u32 {
    let mut voice = bank_voice(supersaw);
    let mut count = 0;
    let mut history = [0; 2];
    let mut quiet = 0u8;
    for _ in 0..48_000 {
        let sample = voice.next_sample()[part];
        if quiet == 0 && sample - history[0] > supersaw.amplitude {
            count += 1;
            // One jump counts once.
            quiet = 4;
        }
        quiet = quiet.saturating_sub(1);
        history = [history[1], sample];
    }
    count
}

#[test]
fn the_centre_and_the_sub_play_their_pitch() {
    let tight = supersaw(440_000, 0, 0);
    let centre = jumps(tight, 0);
    assert!((439..=441).contains(&centre), "{centre}");
    let sub = jumps(tight, 3);
    assert!((219..=221).contains(&sub), "{sub}");
}

#[test]
fn the_parts_go_quiet_with_the_level() {
    let mut voice = bank_voice(Supersaw {
        amplitude: 0,
        ..supersaw(440_000, FULL, FULL)
    });
    for _ in 0..1_000 {
        assert_eq!(voice.next_sample(), [0; 4]);
    }
}
