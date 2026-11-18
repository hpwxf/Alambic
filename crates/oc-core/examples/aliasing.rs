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
//! The level is the default, 1 V: above about 1.1 V the seven voices' sum
//! reaches the outputs' −3 V bound and is clipped, which adds lines that are
//! not harmonics either and would be counted as folding. A second table
//! shows that clipping on its own.
//!
//! ```sh
//! cargo run -p oc-core --example aliasing --release            # the table
//! cargo run -p oc-core --example aliasing --release -- out/    # and a WAV per case
//! ```

use std::f64::consts::TAU;
use std::fs;
use std::io::Write as _;
use std::path::Path;

use oc_core::apps::tone;
use oc_core::audio::supersaw::{Bank, CENTRE, FULL, Supersaw, VOICES, mix_gains};
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

/// The notes measured, C2 to C8 and B7, as MIDI numbers.
const NOTES: [u8; 8] = [36, 48, 60, 72, 84, 96, 107, 108];

/// The detunes measured, in thousandths.
const DETUNES: [u16; 3] = [0, 500, FULL];

/// The mix measured: the default, half way.
const MIX: u16 = FULL / 2;

fn main() {
    let wav_dir = std::env::args().nth(1);
    if let Some(dir) = &wav_dir {
        fs::create_dir_all(dir).expect("the WAV directory must be writable");
    }
    println!(
        "Supersaw at {RATE_HZ} Hz, mix {} %, {LENGTH} samples, Blackman-Harris window.",
        MIX / 10
    );
    println!("Folded power over harmonic power (all, under 10 kHz) and the strongest");
    println!("folded line against the strongest harmonic; PolyBLEP / naive ramps.\n");
    println!(
        "| note | Hz | detune | folded, all (dB) | folded < 10 kHz (dB) | strongest line (dBc) | at (Hz) |"
    );
    println!("|---|---|---|---|---|---|---|");
    for note in NOTES {
        for detune in DETUNES {
            let params = Supersaw {
                frequency: tone::frequency(note, 0),
                detune,
                mix: MIX,
                amplitude: LEVEL_MV,
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
                    "{:.1} / {:.1} | {:.1} / {:.1} | {:.1} / {:.1} | {:.0}",
                    blep.all_db,
                    naive.all_db,
                    blep.audible_db,
                    naive.audible_db,
                    blep.line_dbc,
                    naive.line_dbc,
                    blep.line_hz
                ),
                _ => "— | — | — | (harmonics fill the spectrum)".to_owned(),
            };
            println!(
                "| {} | {hertz:.1} | {} % | {row} |",
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
}

/// The share of samples the outputs' bounds clip, at C4 and full detune, by
/// level and mix.
fn clipping() {
    println!("\nSamples clipped at the outputs' bounds, C4, detune 100 %:\n");
    let mixes: Vec<String> = CLIP_MIXES
        .iter()
        .map(|mix| format!("mix {} %", mix / 10))
        .collect();
    println!("| level | {} |", mixes.join(" | "));
    println!("|---|{}", "---|".repeat(CLIP_MIXES.len()));
    for level in CLIP_LEVELS_MV {
        let shares: Vec<String> = CLIP_MIXES
            .iter()
            .map(|&mix| {
                let params = Supersaw {
                    frequency: tone::frequency(60, 0),
                    detune: FULL,
                    mix,
                    amplitude: level,
                };
                let (played, _) = render(&params);
                let clipped = played
                    .iter()
                    .filter(|&&sample| {
                        sample <= f64::from(CV_OUT_MIN_MV) || sample >= f64::from(CV_OUT_MAX_MV)
                    })
                    .count();
                format!("{:.2} %", 100.0 * count(clipped) / count(played.len()))
            })
            .collect();
        println!(
            "| {:.1} V | {} |",
            f64::from(level) / 1_000.0,
            shares.join(" | ")
        );
    }
}

/// `params` played on the mono part, in millivolts, after the high-pass has
/// settled; and each voice's phase step.
fn render(params: &Supersaw) -> (Vec<f64>, [u32; VOICES]) {
    let mut bank = Bank::new();
    bank.set(params, RATE_HZ);
    for _ in 0..SETTLE {
        bank.next_frame();
    }
    let samples = (0..LENGTH)
        .map(|_| f64::from(bank.next_frame().mono))
        .collect();
    (samples, bank.steps())
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
                let ramp = f64::from(*phase) / 2f64.powi(31) - 1.0;
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

/// `samples` in millivolts as a mono 16-bit WAV at the module's rate, 10 V
/// at full scale.
fn write_wav(path: &Path, samples: &[f64]) {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|&millivolts| {
            #[allow(clippy::cast_possible_truncation)]
            // Within ±10 V, so within an `i16` once scaled.
            let value = (millivolts * 32_767.0 / 10_000.0).round() as i16;
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
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&RATE_HZ.to_le_bytes());
    header.extend_from_slice(&(RATE_HZ * 2).to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&bytes.to_le_bytes());
    file.write_all(&header)
        .and_then(|()| file.write_all(&data))
        .expect("the WAV must be writable");
}
