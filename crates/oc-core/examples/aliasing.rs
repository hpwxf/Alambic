//! How much of the supersaw folds back below the Nyquist frequency, measured
//! on the bank the module plays (lot 8c′, journal/D-037): the decision on
//! oversampling four times (lot 8d) rests on these figures.
//!
//! The bank is the one in `oc_core::audio::supersaw`, integer arithmetic
//! included, rendered at the module's 47 619 Hz and read in millivolts as the
//! DAC receives them; the sawtooths it replaces, naive ramps at the same
//! pitches and gains, are the reference. Each voice's pitch is known exactly
//! from its phase step, so a spectrum separates its harmonics from
//! everything else: what is left is what folded back (and the rounding to
//! the millivolt, far below).
//!
//! The level is the default, 1 V: above about 2 V the seven voices' sum
//! reaches the outputs' +6 V bound, the high-pass's spikes first, and is
//! clipped, which adds lines that are not harmonics either and would be
//! counted as folding. A second table shows that clipping on its own, and the
//! highest level that clips nowhere over every case.
//!
//! ```sh
//! cargo run -p oc-core --example aliasing --release            # the table
//! cargo run -p oc-core --example aliasing --release -- out/    # and the WAVs
//! SUPERSAW_CALC=f32 cargo run -p oc-core --example aliasing --release   # the f32 bank
//! ```
//!
//! `SUPERSAW_CALC=f32` measures the bank in `f32` instead (journal/D-039).
//!
//! A third table compares the bank with an ideal supersaw: each voice built
//! from all its harmonics below the Nyquist frequency, at its exact pitch,
//! gain and starting phase, through the same high-pass, in `f64` and
//! unbounded. Nothing folds, clips or rounds there, so the difference is
//! everything the module adds: folding, the rounding to the millivolt, and
//! the slight loss of the top harmonics `PolyBLEP` costs. It needs no
//! harmonics told apart, so the low notes are measured too.
//!
//! With a directory, it writes a WAV per case of the table, and in `pairs/`
//! what the clipping sounds like: at full detune, the same notes and mixes at
//! 2 V and 3 V (`clipped`) and at 1 V raised as much in the file (`clean`), so
//! both are as loud and differ only where the outputs' bound cut the peaks;
//! `ab` alternates them every two seconds; `ideal`, the ideal supersaw at
//! that level, and `error-clean` and `error-clipped`, each less the ideal, what the
//! module adds without and with the clipping. The files are 32-bit floats,
//! 10 V at full scale.

use std::f64::consts::TAU;
use std::fs;
use std::io::Write as _;
use std::path::Path;

use oc_core::apps::tone;
use oc_core::audio::supersaw::{Arithmetic, Bank, CENTRE, FULL, Supersaw, VOICES, mix_gains};
use oc_core::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};

/// The module's sample rate: PERCLK over 21 (journal/D-033).
const RATE_HZ: u32 = 47_619;

/// Samples analysed: 1.38 s, 0.73 Hz a bin.
const LENGTH: usize = 1 << 16;

/// Samples played before the analysis, for the high-pass to settle.
const SETTLE: usize = 4_800;

/// Bins either side of a harmonic counted as that harmonic: the main lobe of
/// the Blackman-Harris window is four bins wide either side.
const LOBE: usize = 5;

/// Above this share of the spectrum given to harmonics, what is left is too
/// little to measure folding: the low notes.
const MAX_HARMONIC_SHARE: f64 = 0.6;

/// The level the folding is measured at: the default, under the clipping.
const LEVEL_MV: i32 = 1_000;

/// The levels the clipping is measured at.
const CLIP_LEVELS_MV: [i32; 5] = [1_000, 1_500, 2_000, 2_500, 3_000];

/// The mixes the clipping is measured at.
const CLIP_MIXES: [u16; 3] = [0, FULL / 2, FULL];

/// The levels of the pairs' second halves: the cap proposed for `LEVEL`,
/// then the highest.
const PAIR_LEVELS_MV: [i32; 2] = [2_000, 3_000];

/// The notes of the pairs: C3, C4, C6.
const PAIR_NOTES: [u8; 3] = [48, 60, 84];

/// The mixes of the pairs: one sawtooth, then the default.
const PAIR_MIXES: [u16; 2] = [0, FULL / 2];

/// How long a pair's files last, in samples: eight seconds.
const PAIR_LENGTH: usize = 8 * RATE_HZ as usize;

/// How long `ab` plays each half before the other, in samples.
const PAIR_SWITCH: usize = 2 * RATE_HZ as usize;

/// The notes measured, C2 to C8 and B7, as MIDI numbers.
const NOTES: [u8; 8] = [36, 48, 60, 72, 84, 96, 107, 108];

/// The detunes measured, in thousandths.
const DETUNES: [u16; 3] = [0, 500, FULL];

/// The mix measured: the default, half way.
const MIX: u16 = FULL / 2;

/// The bank measured: integers unless `SUPERSAW_CALC` is `f32`.
fn arithmetic() -> Arithmetic {
    match std::env::var("SUPERSAW_CALC").as_deref() {
        Ok("f32") => Arithmetic::Float,
        _ => Arithmetic::Fixed,
    }
}

fn main() {
    println!("Bank: {:?}.", arithmetic());
    let wav_dir = std::env::args().nth(1);
    if let Some(dir) = &wav_dir {
        fs::create_dir_all(dir).expect("the WAV directory must be writable");
    }
    println!(
        "Supersaw at {RATE_HZ} Hz, mix {} %, {LENGTH} samples, Blackman-Harris window.",
        MIX / 10
    );
    println!("Folded power over harmonic power (dB), the whole band and under 10 kHz, and");
    println!("the strongest folded line against the strongest harmonic (dBc), where it is;");
    println!("PolyBLEP, then the naive ramps.\n");
    println!(
        "{:<4} {:>8} {:>6}  {:^15}  {:^15}  {:^15}",
        "", "", "", "folded, all", "folded < 10 kHz", "strongest line"
    );
    println!(
        "{:<4} {:>8} {:>6}  {:>7} {:>7}  {:>7} {:>7}  {:>7} {:>7}  {:>7}",
        "note", "Hz", "detune", "blep", "naive", "blep", "naive", "blep", "naive", "at Hz"
    );
    for note in NOTES {
        println!();
        for detune in DETUNES {
            let params = Supersaw {
                frequency: tone::frequency(note, 0),
                detune,
                mix: MIX,
                amplitude: LEVEL_MV,
                arithmetic: arithmetic(),
            };
            let (played, steps) = render(&params);
            let naive = naive(&params, &steps);
            let frequencies =
                steps.map(|step| f64::from(step) * f64::from(RATE_HZ) / 2f64.powi(32));
            let name = tone::note_name(note);
            let hertz = f64::from(params.frequency) / 1_000.0;
            let row = match (
                measure(&played, &frequencies),
                measure(&naive, &frequencies),
            ) {
                (Some(blep), Some(naive)) => format!(
                    "{:>7.1} {:>7.1}  {:>7.1} {:>7.1}  {:>7.1} {:>7.1}  {:>7.0}",
                    blep.all_db,
                    naive.all_db,
                    blep.audible_db,
                    naive.audible_db,
                    blep.line_dbc,
                    naive.line_dbc,
                    blep.line_hz
                ),
                _ => format!(
                    "{:>15}  {:>15}  {:>15}  {:>7}   harmonics fill the spectrum",
                    "—", "—", "—", "—"
                ),
            };
            println!(
                "{:<4} {hertz:>8.1} {:>4} %  {row}",
                name.as_str(),
                detune / 10
            );
            if let Some(dir) = &wav_dir {
                let path =
                    Path::new(dir).join(format!("supersaw-{}-{}.wav", name.as_str(), detune / 10));
                write_wav(&path, &played);
            }
        }
    }
    clipping();
    against_ideal();
    if let Some(dir) = &wav_dir {
        write_pairs(&Path::new(dir).join("pairs"));
    }
}

/// The clipping pairs (see the module's documentation) into `dir`.
fn write_pairs(dir: &Path) {
    fs::create_dir_all(dir).expect("the pairs' directory must be writable");
    for note in PAIR_NOTES {
        for (mix, level) in PAIR_MIXES
            .iter()
            .flat_map(|&mix| PAIR_LEVELS_MV.map(|level| (mix, level)))
        {
            let gain = f64::from(level) / f64::from(LEVEL_MV);
            let at = |amplitude| Supersaw {
                frequency: tone::frequency(note, 0),
                detune: FULL,
                mix,
                amplitude,
                arithmetic: arithmetic(),
            };
            let clean: Vec<f64> = play(&at(LEVEL_MV), PAIR_LENGTH)
                .iter()
                .map(|sample| sample * gain)
                .collect();
            let (clipped, steps, phases) = render_for(&at(level), PAIR_LENGTH);
            let ideal = ideal(&at(level), &steps, &phases, PAIR_LENGTH);
            let error_clean = difference(&clean, &ideal);
            let error_clipped = difference(&clipped, &ideal);
            let ab: Vec<f64> = clean
                .iter()
                .zip(&clipped)
                .enumerate()
                .map(|(index, (&clean, &clipped))| {
                    if (index / PAIR_SWITCH).is_multiple_of(2) {
                        clean
                    } else {
                        clipped
                    }
                })
                .collect();
            let name = format!(
                "{}-mix{}-{}V",
                tone::note_name(note).as_str(),
                mix / 10,
                level / 1_000
            );
            for (kind, samples) in [
                ("clean", &clean),
                ("clipped", &clipped),
                ("ab", &ab),
                ("ideal", &ideal),
                ("error-clean", &error_clean),
                ("error-clipped", &error_clipped),
            ] {
                write_wav(&dir.join(format!("{name}-{kind}.wav")), samples);
            }
            println!(
                "{name:<13} error against the ideal: {:>6.1} dB clean, {:>6.1} dB clipped",
                power_db(&error_clean, &ideal),
                power_db(&error_clipped, &ideal)
            );
        }
    }
    println!("Clipping pairs written to {}", dir.display());
}

/// The difference between the module and the ideal, by note and detune:
/// its power against the ideal's, over the whole band and under 10 kHz.
fn against_ideal() {
    println!("\nThe module less the ideal supersaw, against the ideal, mix 50 %, 1 V:\n");
    println!(
        "{:<4} {:>8} {:>6}  {:>9} {:>9}",
        "note", "Hz", "detune", "all", "< 10 kHz"
    );
    for note in NOTES {
        println!();
        for detune in DETUNES {
            let params = Supersaw {
                frequency: tone::frequency(note, 0),
                detune,
                mix: MIX,
                amplitude: LEVEL_MV,
                arithmetic: arithmetic(),
            };
            let (played, steps, phases) = render_for(&params, LENGTH);
            let ideal = ideal(&params, &steps, &phases, LENGTH);
            let error = spectrum(&difference(&played, &ideal));
            let reference = spectrum(&ideal);
            let bin_hz = f64::from(RATE_HZ) / count(LENGTH);
            let audible = bin_of(10_000.0 / bin_hz);
            let total = |power: &[f64]| power.iter().sum::<f64>();
            let db = |ratio: f64| 10.0 * ratio.log10();
            println!(
                "{:<4} {:>8.1} {:>4} %  {:>6.1} dB {:>6.1} dB",
                tone::note_name(note).as_str(),
                f64::from(params.frequency) / 1_000.0,
                detune / 10,
                db(total(&error) / total(&reference)),
                db(total(&error[..audible]) / total(&reference[..audible]))
            );
        }
    }
}

/// The share of samples the outputs' bounds clip, at C4 and full detune, by
/// level and mix.
fn clipping() {
    println!("\nSamples clipped at the outputs' bounds, C4, detune 100 %:\n");
    let mixes: Vec<String> = CLIP_MIXES
        .iter()
        .map(|mix| format!("{:>10}", format!("mix {} %", mix / 10)))
        .collect();
    println!("{:>7}  {}", "level", mixes.join(" "));
    for level in CLIP_LEVELS_MV {
        let shares: Vec<String> = CLIP_MIXES
            .iter()
            .map(|&mix| {
                let params = Supersaw {
                    frequency: tone::frequency(60, 0),
                    detune: FULL,
                    mix,
                    amplitude: level,
                    arithmetic: arithmetic(),
                };
                let (played, _) = render(&params);
                let clipped = played
                    .iter()
                    .filter(|&&sample| {
                        sample <= f64::from(CV_OUT_MIN_MV) || sample >= f64::from(CV_OUT_MAX_MV)
                    })
                    .count();
                format!("{:>8.2} %", 100.0 * count(clipped) / count(played.len()))
            })
            .collect();
        println!(
            "{:>5.1} V  {}",
            f64::from(level) / 1_000.0,
            shares.join(" ")
        );
    }
    let highest: Vec<String> = CLIP_MIXES
        .iter()
        .map(|&mix| {
            let params = Supersaw {
                frequency: tone::frequency(60, 0),
                detune: FULL,
                mix,
                amplitude: LEVEL_MV,
                arithmetic: arithmetic(),
            };
            let (played, _) = render(&params);
            let low = played.iter().copied().fold(0.0, f64::min);
            let high = played.iter().copied().fold(0.0, f64::max);
            // The level scales the samples: the bound reached first sets it.
            let room = (f64::from(CV_OUT_MIN_MV) / low).min(f64::from(CV_OUT_MAX_MV) / high);
            format!("{:>8.2} V", room * f64::from(LEVEL_MV) / 1_000.0)
        })
        .collect();
    println!("{:>7}  {}", "highest", highest.join(" "));
    worst_level();
}

/// The highest level that clips nowhere: over every note, detune and mix
/// measured, eight seconds each, the least room either bound leaves.
fn worst_level() {
    let mut worst = (f64::MAX, 0, 0, 0);
    for note in NOTES {
        for detune in DETUNES {
            for mix in CLIP_MIXES {
                let params = Supersaw {
                    frequency: tone::frequency(note, 0),
                    detune,
                    mix,
                    amplitude: LEVEL_MV,
                    arithmetic: arithmetic(),
                };
                let played = play(&params, PAIR_LENGTH);
                let low = played.iter().copied().fold(0.0, f64::min);
                let high = played.iter().copied().fold(0.0, f64::max);
                let room = (f64::from(CV_OUT_MIN_MV) / low).min(f64::from(CV_OUT_MAX_MV) / high);
                if room < worst.0 {
                    worst = (room, note, detune, mix);
                }
            }
        }
    }
    let (room, note, detune, mix) = worst;
    println!(
        "\nHighest level clipping nowhere, every case above over eight seconds: {:.2} V \
         (the least room at {}, detune {} %, mix {} %)",
        room * f64::from(LEVEL_MV) / 1_000.0,
        tone::note_name(note).as_str(),
        detune / 10,
        mix / 10
    );
}

/// `params` played on the mono part, in millivolts, after the high-pass has
/// settled; and each voice's phase step.
fn render(params: &Supersaw) -> (Vec<f64>, [u32; VOICES]) {
    let (samples, steps, _) = render_for(params, LENGTH);
    (samples, steps)
}

/// `length` samples of `params`, as [`render`] plays them, with each voice's
/// step and starting phase.
fn render_for(params: &Supersaw, length: usize) -> (Vec<f64>, [u32; VOICES], [u32; VOICES]) {
    let mut bank = Bank::new();
    bank.set(params, RATE_HZ);
    let phases = bank.phases();
    (played(&mut bank, length), bank.steps(), phases)
}

/// The ideal supersaw the bank approaches: from `phases`, at `steps`, each
/// voice the sum of its harmonics below the Nyquist frequency, the same gains,
/// normalisation and high-pass, in `f64` and unbounded; `length` samples
/// after the same settling, in millivolts.
fn ideal(
    params: &Supersaw,
    steps: &[u32; VOICES],
    phases: &[u32; VOICES],
    length: usize,
) -> Vec<f64> {
    let turn = 2f64.powi(32);
    let (centre, side) = mix_gains(params.mix);
    let gains: Vec<f64> = (0..VOICES)
        .map(|index| if index == CENTRE { centre } else { side })
        .map(|gain| i32::try_from(gain).map_or(0.0, f64::from))
        .collect();
    let norm = gains.iter().map(|gain| gain * gain).sum::<f64>().sqrt();
    // The harmonics of each voice below the Nyquist frequency.
    let harmonics: Vec<u32> = steps
        .iter()
        .map(|&step| {
            let cycles = f64::from(step) / turn;
            let mut count = 0;
            while f64::from(count + 1) * cycles < 0.5 {
                count += 1;
            }
            count
        })
        .collect();
    // The bank's one pole, on the fundamental's step.
    let coefficient = 1.0 / (1.0 + TAU * f64::from(steps[CENTRE]) / turn);
    let (mut previous, mut output) = (0.0, 0.0);
    let mut samples = Vec::with_capacity(length);
    for index in 0..SETTLE + length {
        let mut sum = 0.0;
        for voice in 0..VOICES {
            #[allow(clippy::cast_possible_truncation)]
            // The phase wraps round a turn, as the bank's does.
            let phase = phases[voice].wrapping_add(steps[voice].wrapping_mul(index as u32));
            sum +=
                gains[voice] * band_limited_ramp(TAU * f64::from(phase) / turn, harmonics[voice]);
        }
        let input = sum / norm;
        output = coefficient * (output + input - previous);
        previous = input;
        if index >= SETTLE {
            samples.push(output * f64::from(params.amplitude));
        }
    }
    samples
}

/// A falling ramp from 1 to −1 at `angle`, as the bank plays its voices, of
/// its first `harmonics` harmonics: `(2 / pi) sum sin(k angle) / k`, by the
/// sines' recurrence.
fn band_limited_ramp(angle: f64, harmonics: u32) -> f64 {
    let twice_cos = 2.0 * angle.cos();
    let (mut before, mut sine) = (0.0, angle.sin());
    let mut sum = 0.0;
    for k in 1..=harmonics {
        sum += sine / f64::from(k);
        let next = twice_cos * sine - before;
        before = sine;
        sine = next;
    }
    sum / std::f64::consts::FRAC_PI_2
}

/// `samples` less `reference`, sample by sample.
fn difference(samples: &[f64], reference: &[f64]) -> Vec<f64> {
    samples.iter().zip(reference).map(|(a, b)| a - b).collect()
}

/// The power of `error` against the power of `reference`, in decibels.
fn power_db(error: &[f64], reference: &[f64]) -> f64 {
    let power = |samples: &[f64]| samples.iter().map(|value| value * value).sum::<f64>();
    10.0 * (power(error) / power(reference)).log10()
}

/// `length` samples of `params` on the mono part, as [`render`] plays them.
fn play(params: &Supersaw, length: usize) -> Vec<f64> {
    let mut bank = Bank::new();
    bank.set(params, RATE_HZ);
    played(&mut bank, length)
}

/// `length` samples of the mono part of `bank`, in millivolts, after the
/// high-pass has settled.
fn played(bank: &mut Bank, length: usize) -> Vec<f64> {
    for _ in 0..SETTLE {
        bank.next_frame();
    }
    (0..length)
        .map(|_| f64::from(bank.next_frame().mono))
        .collect()
}

/// The same voices as naive ramps, at the same steps and gains, unfiltered.
fn naive(params: &Supersaw, steps: &[u32; VOICES]) -> Vec<f64> {
    let (centre, side) = mix_gains(params.mix);
    let gains: Vec<f64> = (0..VOICES)
        .map(|index| if index == CENTRE { centre } else { side })
        .map(|gain| i32::try_from(gain).map_or(0.0, f64::from))
        .collect();
    let mut phases = [0u32; VOICES];
    (0..SETTLE + LENGTH)
        .map(|_| {
            let mut sum = 0.0;
            for ((phase, step), gain) in phases.iter_mut().zip(steps).zip(&gains) {
                // Falling, as the bank plays them.
                let ramp = 1.0 - f64::from(*phase) / 2f64.powi(31);
                sum += ramp * gain;
                *phase = phase.wrapping_add(*step);
            }
            sum
        })
        .skip(SETTLE)
        .collect()
}

/// The folding of one signal, in decibels.
struct Folding {
    /// Folded power over harmonic power, the whole band.
    all_db: f64,
    /// The same, folded power under 10 kHz only.
    audible_db: f64,
    /// The strongest folded bin against the strongest harmonic one.
    line_dbc: f64,
    /// Where that bin is.
    line_hz: f64,
}

/// How much of `samples` is not a harmonic of `frequencies`; `None` when the
/// harmonics leave too little of the spectrum to tell.
fn measure(samples: &[f64], frequencies: &[f64; VOICES]) -> Option<Folding> {
    let power = spectrum(samples);
    let bin_hz = f64::from(RATE_HZ) / count(LENGTH);
    let half = LENGTH / 2;
    let mut harmonic = vec![false; half];
    harmonic[..=LOBE].fill(true);
    for &frequency in frequencies {
        let mut multiple = frequency;
        while multiple < f64::from(RATE_HZ) / 2.0 {
            let centre = bin_of(multiple / bin_hz);
            let low = centre.saturating_sub(LOBE);
            let high = (centre + LOBE).min(half - 1);
            harmonic[low..=high].fill(true);
            multiple += frequency;
        }
    }
    let share = count(harmonic.iter().filter(|&&taken| taken).count()) / count(half);
    if share > MAX_HARMONIC_SHARE {
        return None;
    }
    let audible = bin_of(10_000.0 / bin_hz);
    let (mut wanted, mut folded, mut folded_audible) = (0.0, 0.0, 0.0);
    let (mut strongest_wanted, mut strongest_folded, mut strongest_bin) = (0.0f64, 0.0f64, 0);
    for (bin, (&value, &taken)) in power.iter().zip(&harmonic).enumerate() {
        if taken {
            wanted += value;
            strongest_wanted = strongest_wanted.max(value);
        } else {
            folded += value;
            if value > strongest_folded {
                strongest_folded = value;
                strongest_bin = bin;
            }
            if bin < audible {
                folded_audible += value;
            }
        }
    }
    let db = |ratio: f64| 10.0 * ratio.log10();
    Some(Folding {
        all_db: db(folded / wanted),
        audible_db: db(folded_audible / wanted),
        line_dbc: db(strongest_folded / strongest_wanted),
        line_hz: count(strongest_bin) * bin_hz,
    })
}

/// The power of each bin up to the Nyquist frequency, through a
/// Blackman-Harris window.
fn spectrum(samples: &[f64]) -> Vec<f64> {
    let length = count(samples.len());
    let mut values: Vec<(f64, f64)> = samples
        .iter()
        .enumerate()
        .map(|(index, &sample)| {
            let x = TAU * count(index) / length;
            let window = 0.358_75 - 0.488_29 * x.cos() + 0.141_28 * (2.0 * x).cos()
                - 0.011_68 * (3.0 * x).cos();
            (sample * window, 0.0)
        })
        .collect();
    fft(&mut values);
    values[..samples.len() / 2]
        .iter()
        .map(|&(re, im)| re * re + im * im)
        .collect()
}

/// In-place radix-2 FFT; the length is a power of two.
fn fft(values: &mut [(f64, f64)]) {
    let length = values.len();
    let mut j = 0;
    for i in 1..length {
        let mut bit = length >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            values.swap(i, j);
        }
    }
    let mut size = 2;
    while size <= length {
        let angle = -TAU / count(size);
        for start in (0..length).step_by(size) {
            for k in 0..size / 2 {
                let (sin, cos) = (angle * count(k)).sin_cos();
                let (re, im) = values[start + k + size / 2];
                let turned = (re * cos - im * sin, re * sin + im * cos);
                let even = values[start + k];
                values[start + k] = (even.0 + turned.0, even.1 + turned.1);
                values[start + k + size / 2] = (even.0 - turned.0, even.1 - turned.1);
            }
        }
        size <<= 1;
    }
}

/// `value` as a float, for indices and lengths far below `2^52`.
#[allow(clippy::cast_precision_loss)]
fn count(value: usize) -> f64 {
    value as f64
}

/// The nearest bin to a position in bins, never negative.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
// A position within the spectrum: positive and far below `usize::MAX`.
fn bin_of(position: f64) -> usize {
    position.round().max(0.0) as usize
}

/// `samples` in millivolts as a mono 32-bit float WAV at the module's rate,
/// 10 V at full scale: the floats keep what an error file holds, far below
/// a 16-bit step.
fn write_wav(path: &Path, samples: &[f64]) {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|&millivolts| {
            #[allow(clippy::cast_possible_truncation)]
            // A level within a few units of full scale fits a `f32`.
            let value = (millivolts / 10_000.0) as f32;
            value.to_le_bytes()
        })
        .collect();
    let bytes = u32::try_from(data.len()).expect("a WAV under 4 GiB");
    let mut file = fs::File::create(path).expect("the WAV must be writable");
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    // IEEE float, one channel.
    header.extend_from_slice(&3u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&RATE_HZ.to_le_bytes());
    header.extend_from_slice(&(RATE_HZ * 4).to_le_bytes());
    header.extend_from_slice(&4u16.to_le_bytes());
    header.extend_from_slice(&32u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&bytes.to_le_bytes());
    file.write_all(&header)
        .and_then(|()| file.write_all(&data))
        .expect("the WAV must be writable");
}
