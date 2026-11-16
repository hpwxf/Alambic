//! `oc-probe`: the command line over the module's probe port.

use std::fs::File;
use std::io::{self, BufWriter};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Context as _;
use clap::{Args, Parser, Subcommand};

use oc_core::platform::{CV_CHANNELS, CvChannel};
use oc_probe::client::ProbeError;
use oc_probe::port::{self, Session};
use oc_probe::{measure, render};
use oc_protocol::Sample;

/// Talks to a running Ornament & Crime module (Alambic firmware) over the
/// second of its USB serial ports.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    /// The probe port; found by trying each candidate when left out.
    #[arg(long, global = true)]
    port: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Lists the serial ports that may be the module's.
    Ports,
    /// Says hello: firmware version and build.
    Info,
    /// Prints where the loop's time goes.
    Stats {
        /// Clears the profile first, then waits this many seconds.
        #[arg(long, value_name = "SECONDS")]
        fresh: Option<u64>,
        /// Prints it again every this many seconds, until interrupted.
        #[arg(long, value_name = "SECONDS")]
        watch: Option<u64>,
    },
    /// Prints the inputs and outputs, one line per sample.
    Stream {
        /// Ticks between two samples (1 ms each).
        #[arg(long, default_value_t = 100)]
        every: u16,
        /// Stops after this many samples; never when left out.
        #[arg(long)]
        count: Option<u64>,
    },
    /// Holds an output at a level until Enter, printing the inputs.
    Out {
        /// The output: A, B, C or D (or 1 to 4).
        channel: String,
        /// The level in millivolts.
        #[arg(allow_hyphen_values = true)]
        millivolts: i32,
    },
    /// Reads every input for a while: extremes, mean, peak to peak, codes
    /// (`TESTING.md` 9.2).
    Inputs {
        /// How long, in seconds of module time.
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
    /// Counts the rising edges of every trigger, sampled on every tick
    /// (`TESTING.md` 9.3).
    Triggers {
        /// How long, in seconds of module time.
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
    /// Steps an output through levels and reads an input patched to it:
    /// the error of the two paths together.
    Sweep(SweepArgs),
    /// Captures the screen.
    Screen {
        /// Writes a PNG here instead of printing text.
        #[arg(long, value_name = "FILE")]
        png: Option<String>,
        /// Pixel size in the PNG.
        #[arg(long, default_value_t = 4)]
        scale: u32,
    },
    /// Restarts the module into its bootloader, ready to be flashed.
    Reboot,
}

/// What `sweep` steps through.
#[derive(Debug, Args)]
struct SweepArgs {
    /// The output: A, B, C or D.
    #[arg(long = "out", default_value = "A")]
    output: String,
    /// The input it is patched to: 1 to 4.
    #[arg(long = "in", default_value_t = 1)]
    input: usize,
    /// First level, mV.
    #[arg(long, default_value_t = -3_000, allow_hyphen_values = true)]
    from: i32,
    /// Last level, mV.
    #[arg(long, default_value_t = 6_000, allow_hyphen_values = true)]
    to: i32,
    /// Step, mV.
    #[arg(long, default_value_t = 1_000)]
    step: i32,
    /// Ticks left to settle after each change.
    #[arg(long, default_value_t = 50)]
    settle: u64,
    /// Ticks averaged at each level.
    #[arg(long, default_value_t = 200)]
    average: u64,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Command::Ports = cli.command {
        for path in port::candidates()? {
            println!("{path}");
        }
        return Ok(());
    }
    let (path, mut session, info) = port::find(cli.port.as_deref())?;
    eprintln!("{path}: oc-firmware {} ({})", info.firmware, info.build);

    match cli.command {
        Command::Ports | Command::Info => Ok(()),
        Command::Stats { fresh, watch } => stats(&mut session, fresh, watch),
        Command::Stream { every, count } => stream(&mut session, every, count),
        Command::Out {
            channel,
            millivolts,
        } => hold(&mut session, &channel, millivolts),
        Command::Inputs { seconds } => inputs(&mut session, seconds),
        Command::Triggers { seconds } => triggers(&mut session, seconds),
        Command::Sweep(args) => sweep(&mut session, &args),
        Command::Screen { png, scale } => screen(&mut session, png, scale),
        Command::Reboot => {
            session.reboot_to_loader()?;
            eprintln!("restarting into the bootloader");
            Ok(())
        }
    }
}

fn stats(session: &mut Session, fresh: Option<u64>, watch: Option<u64>) -> anyhow::Result<()> {
    if let Some(seconds) = fresh {
        session.clear_profile()?;
        thread::sleep(Duration::from_secs(seconds));
    }
    loop {
        print!("{}", render::profile_table(&session.profile()?));
        let Some(seconds) = watch else {
            return Ok(());
        };
        thread::sleep(Duration::from_secs(seconds));
        println!();
    }
}

fn stream(session: &mut Session, every: u16, count: Option<u64>) -> anyhow::Result<()> {
    session.stream(every.max(1))?;
    let mut printed = 0;
    while count.is_none_or(|count| printed < count) {
        match session.next_sample() {
            Ok(sample) => println!("{}", render::sample_line(&sample)),
            // A slow stream may leave a read empty: keep waiting.
            Err(ProbeError::Timeout) => continue,
            Err(error) => return Err(error.into()),
        }
        printed += 1;
    }
    session.stream(0)?;
    Ok(())
}

fn hold(session: &mut Session, channel: &str, millivolts: i32) -> anyhow::Result<()> {
    let channel = parse_channel(channel)?;
    session.force_output(channel, Some(millivolts))?;
    session.stream(100)?;
    eprintln!("output {channel:?} held at {millivolts} mV; Enter releases it");
    let enter = enter_pressed();
    while enter.try_recv().is_err() {
        match session.next_sample() {
            Ok(sample) => println!("{}", render::sample_line(&sample)),
            Err(ProbeError::Timeout) => {}
            Err(error) => return Err(error.into()),
        }
    }
    session.stream(0)?;
    session.force_output(channel, None)?;
    Ok(())
}

fn inputs(session: &mut Session, seconds: u64) -> anyhow::Result<()> {
    let ticks = seconds * ticks_per_second(session)?;
    session.stream(1)?;
    let samples = collect(session, ticks)?;
    session.stream(0)?;
    let spread = measure::input_spread(&samples).context("no sample came")?;
    println!("{} samples", samples.len());
    println!("in       min mV   max mV  mean mV    pp mV   codes");
    for (index, input) in spread.iter().enumerate() {
        println!(
            "CV{}    {:>7}  {:>7}  {:>7}  {:>7}   {}..{}",
            index + 1,
            input.min_mv,
            input.max_mv,
            input.mean_mv,
            input.peak_to_peak_mv(),
            input.min_code,
            input.max_code
        );
    }
    Ok(())
}

fn triggers(session: &mut Session, seconds: u64) -> anyhow::Result<()> {
    let ticks = seconds * ticks_per_second(session)?;
    session.stream(1)?;
    eprintln!("counting for {seconds} s of module time");
    let samples = collect(session, ticks)?;
    session.stream(0)?;
    let count = measure::count_edges(&samples);
    for (index, (edges, intervals)) in count.rising.iter().zip(&count.intervals).enumerate() {
        match intervals.mean() {
            Some(mean) => println!(
                "TR{}  {edges:>6} rising edges, ticks between them {}..{} (mean {mean:.2})",
                index + 1,
                intervals.min,
                intervals.max
            ),
            None => println!("TR{}  {edges:>6} rising edges", index + 1),
        }
    }
    if count.missed_ticks > 0 {
        println!(
            "INEXACT: {} of {} ticks had no sample (the host fell behind)",
            count.missed_ticks, count.ticks
        );
    }
    Ok(())
}

fn sweep(session: &mut Session, args: &SweepArgs) -> anyhow::Result<()> {
    let channel = parse_channel(&args.output)?;
    let input = args
        .input
        .checked_sub(1)
        .filter(|&index| index < CV_CHANNELS)
        .context("no input with that number: 1 to 4")?;
    session.stream(1)?;
    println!("out mV    in mV  error mV  code");
    let mut steps = || -> anyhow::Result<()> {
        for level in measure::sweep_levels(args.from, args.to, args.step) {
            session.force_output(channel, Some(level))?;
            let _ = collect(session, args.settle)?;
            let settled = collect(session, args.average)?;
            let point = measure::sweep_point(level, input, &settled).context("no sample came")?;
            println!(
                "{:>6}  {:>7}  {:>8}  {:>4}",
                point.out_mv,
                point.in_mv,
                point.error_mv(),
                point.in_code
            );
        }
        Ok(())
    };
    let result = steps();
    // Released whatever happened, so a failed sweep leaves no level held.
    session.stream(0)?;
    session.force_output(channel, None)?;
    result
}

fn screen(session: &mut Session, png: Option<String>, scale: u32) -> anyhow::Result<()> {
    let screen = session.screen()?;
    match png {
        Some(file) => {
            let out = BufWriter::new(File::create(&file).with_context(|| file.clone())?);
            render::screen_png(&screen, scale, out)?;
            eprintln!("written {file}");
        }
        None => print!("{}", render::screen_text(&screen)),
    }
    Ok(())
}

/// Ticks a second, from the period the module's loop aims at (samples are
/// numbered in ticks); a millisecond if it does not say.
fn ticks_per_second(session: &mut Session) -> anyhow::Result<u64> {
    let period = u64::from(session.profile()?.expected_period_micros());
    Ok(1_000_000_u64.checked_div(period).unwrap_or(1_000))
}

/// Samples covering `ticks` ticks of module time, from the next one on.
fn collect(session: &mut Session, ticks: u64) -> anyhow::Result<Vec<Sample>> {
    let mut samples: Vec<Sample> = Vec::new();
    loop {
        let sample = match session.next_sample() {
            Ok(sample) => sample,
            Err(ProbeError::Timeout) => continue,
            Err(error) => return Err(error.into()),
        };
        if let Some(first) = samples.first()
            && sample.tick.saturating_sub(first.tick) >= ticks
        {
            return Ok(samples);
        }
        samples.push(sample);
    }
}

/// `A`..`D` or `1`..`4`.
fn parse_channel(text: &str) -> anyhow::Result<CvChannel> {
    let index = match text.to_ascii_uppercase().as_str() {
        "A" | "1" => 0,
        "B" | "2" => 1,
        "C" | "3" => 2,
        "D" | "4" => 3,
        _ => anyhow::bail!("no output {text}: A, B, C or D"),
    };
    Ok(CvChannel::ALL[index])
}

/// Signals once Enter is pressed on the terminal.
fn enter_pressed() -> mpsc::Receiver<()> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
        let _ = sender.send(());
    });
    receiver
}
