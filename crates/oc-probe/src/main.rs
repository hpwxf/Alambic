//! `oc-probe`: the command line over the module's probe port.

use std::fs::File;
use std::io::{self, BufWriter};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Context as _;
use clap::{Parser, Subcommand};

use oc_core::platform::CvChannel;
use oc_probe::client::ProbeError;
use oc_probe::{port, render};

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
        Command::Ports | Command::Info => {}
        Command::Stats { fresh, watch } => {
            if let Some(seconds) = fresh {
                session.clear_profile()?;
                thread::sleep(Duration::from_secs(seconds));
            }
            loop {
                print!("{}", render::profile_table(&session.profile()?));
                let Some(seconds) = watch else { break };
                thread::sleep(Duration::from_secs(seconds));
                println!();
            }
        }
        Command::Stream { every, count } => {
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
        }
        Command::Out {
            channel,
            millivolts,
        } => {
            let channel = parse_channel(&channel)?;
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
        }
        Command::Screen { png, scale } => {
            let screen = session.screen()?;
            match png {
                Some(file) => {
                    let out = BufWriter::new(File::create(&file).with_context(|| file.clone())?);
                    render::screen_png(&screen, scale, out)?;
                    eprintln!("written {file}");
                }
                None => print!("{}", render::screen_text(&screen)),
            }
        }
        Command::Reboot => {
            session.reboot_to_loader()?;
            eprintln!("restarting into the bootloader");
        }
    }
    Ok(())
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
