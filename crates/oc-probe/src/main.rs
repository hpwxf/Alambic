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
use oc_probe::client::{AppDescription, AppEntry, InstanceEntry};
use oc_probe::port::{self, Session};
use oc_probe::{apps, measure, render};
use oc_protocol::{Sample, Target};

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
        /// How long, in seconds of the module's clock.
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
    /// Counts the rising edges of every trigger, sampled on every tick
    /// (`TESTING.md` 9.3).
    Triggers {
        /// How long, in seconds of the module's clock.
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
    /// Lists the applets; the running one is marked (journal/D-043).
    Apps,
    /// Describes an applet: its parameters with their values and ranges,
    /// what it can be read for, what it can be asked to do, its ports.
    App {
        /// The applet: its name (`ratchet`, `gfx-demo`) or its index, its
        /// first instance then; or `@N`, the instance at position N
        /// (journal/D-046). The same for every command below.
        app: String,
    },
    /// Reads one parameter or observable of an applet.
    Get {
        /// The applet: its name or its index.
        app: String,
        /// The parameter or observable: its name or its index.
        name: String,
    },
    /// Sets one parameter of an applet, within the bounds the panel uses,
    /// and prints what it applied.
    Set {
        /// The applet: its name or its index.
        app: String,
        /// The parameter: its name or its index.
        param: String,
        /// The value: `-1.5`, `A4`, `span`, `on`…
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    /// Asks an applet for one of its actions (`ratchet burst`).
    Act {
        /// The applet: its name or its index.
        app: String,
        /// The action: its name or its index.
        action: String,
    },
    /// Hands the outputs and the screen to an applet, as the app menu does.
    Select {
        /// The applet: its name or its index.
        app: String,
    },
    /// Reads the CV input of an applet's parameter, or sets it field by
    /// field as its submenu does, and prints what it applied
    /// (journal/D-045).
    Assign(AssignArgs),
    /// Lists the applets running, left to right, with where their ports go;
    /// the one shown is marked (journal/D-046).
    Instances,
    /// Starts an applet beside the one shown, on the free outputs, as the
    /// menu's `ADD APP` does.
    Add {
        /// The applet: its name or its index.
        app: String,
    },
    /// Stops an instance, freeing its outputs.
    Remove {
        /// The instance: `@N`.
        instance: String,
    },
    /// Shows an instance on the screen and gives it the panel.
    Focus {
        /// The instance: `@N`.
        instance: String,
    },
    /// Prints where an instance's ports go, after routing those given, as
    /// its routing page does: `sub=off`, `a=d`, `clock=tr2`.
    Route {
        /// The instance: `@N`.
        instance: String,
        /// Ports to route, as `port=jack`.
        #[arg(value_name = "PORT=JACK")]
        routes: Vec<String>,
    },
}

/// What `assign` reads or changes.
#[derive(Debug, Args)]
struct AssignArgs {
    /// The applet: its name or its index.
    app: String,
    /// The parameter: its name or its index.
    param: String,
    /// Fields to change, as `field=value`: `input=cv2` (or `none`),
    /// `effect=add` (`set`, `mul`), `scale=0.5`, `shift=-0.25`, `min=0`,
    /// `max=5` (volts); the others are kept.
    #[arg(allow_hyphen_values = true, value_name = "FIELD=VALUE")]
    fields: Vec<String>,
    /// Then fits the scale and shift to what the input carries over this
    /// many seconds, as the submenu's `CENTER` does.
    #[arg(long, value_name = "SECONDS")]
    center: Option<u64>,
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
    /// Milliseconds left to settle after each change.
    #[arg(long, default_value_t = 50)]
    settle_ms: u64,
    /// Milliseconds averaged at each level.
    #[arg(long, default_value_t = 200)]
    average_ms: u64,
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
        Command::Instances => {
            for instance in session.instances()? {
                let description = session.describe(Target::Instance(instance.position))?;
                println!("{}", instance_line(&mut session, &instance)?);
                for line in apps::routing_text(&description, &instance).lines() {
                    println!("    {line}");
                }
            }
            Ok(())
        }
        Command::Add { app } => {
            let app = found(&mut session, &app)?;
            let instance = session.add_instance(app.index)?;
            println!("{}", instance_line(&mut session, &instance)?);
            Ok(())
        }
        Command::Remove { instance } => {
            session.remove_instance(position(&instance)?)?;
            Ok(())
        }
        Command::Focus { instance } => {
            session.focus(position(&instance)?)?;
            Ok(())
        }
        Command::Route { instance, routes } => route(&mut session, &instance, &routes),
        Command::Apps => {
            for app in session.apps()? {
                println!(
                    "{} {} {}",
                    if app.running { '*' } else { ' ' },
                    app.index,
                    app.name
                );
            }
            Ok(())
        }
        Command::App { app } => {
            let (app, description) = described(&mut session, &app)?;
            print!("{}", apps::description_text(&app.entry, &description));
            Ok(())
        }
        Command::Get { app, name } => get(&mut session, &app, &name),
        Command::Set { app, param, value } => set(&mut session, &app, &param, &value),
        Command::Act { app, action } => {
            let (app, description) = described(&mut session, &app)?;
            let index = apps::find_action(&description.actions, &action).with_context(|| {
                format!(
                    "{} has no action {action}; it has: {}",
                    app.entry.name,
                    description.actions.join(", ")
                )
            })?;
            session.act(app.target, index)?;
            Ok(())
        }
        Command::Select { app } => {
            let app = found(&mut session, &app)?;
            session.select_app(app.index)?;
            eprintln!("{} drives the outputs and the screen", app.name);
            Ok(())
        }
        Command::Assign(args) => assign(&mut session, &args),
    }
}

/// Prints a parameter's CV input, after changing the fields given and
/// fitting it to its signal when asked.
fn assign(session: &mut Session, args: &AssignArgs) -> anyhow::Result<()> {
    let (app, description) = described(session, &args.app)?;
    let spec = apps::find_spec(&description.params, &args.param)
        .with_context(|| format!("{} has no parameter {}", app.entry.name, args.param))?;
    let mut input = spec
        .input
        .with_context(|| format!("{} {} takes no CV input", app.entry.name, spec.label))?;
    if !args.fields.is_empty() {
        let mut assign = input.assign;
        for field in &args.fields {
            let (name, text) = field
                .split_once('=')
                .with_context(|| format!("{field}: write it field=value"))?;
            assign = apps::parse_input_field(assign, name, text).map_err(anyhow::Error::msg)?;
        }
        input = session.set_assign(app.target, spec.index, assign)?;
    }
    if let Some(seconds) = args.center {
        session.stream(1)?;
        eprintln!("reading the input for {seconds} s of the module's clock");
        let samples = collect(session, seconds * 1_000_000)?;
        session.stream(0)?;
        let assign = apps::centered(&input, &samples).map_err(anyhow::Error::msg)?;
        input = session.set_assign(app.target, spec.index, assign)?;
    }
    println!("{} CV {}", spec.label, apps::input_text(&input.assign));
    Ok(())
}

/// The applet `query` names on the module.
fn found(session: &mut Session, query: &str) -> anyhow::Result<AppEntry> {
    let list = session.apps()?;
    apps::find_app(&list, query).cloned().with_context(|| {
        let names: Vec<_> = list.iter().map(|app| app.name.as_str()).collect();
        format!("no applet {query}; there are: {}", names.join(", "))
    })
}

/// An applet as a command names it: what it is, and how the requests reach
/// it.
struct Addressed {
    entry: AppEntry,
    target: Target,
}

/// The instance position `query` names, written `@N`.
fn position(query: &str) -> anyhow::Result<u8> {
    apps::instance_position(query).with_context(|| format!("{query}: an instance is @N"))
}

/// The applet `query` names: `@N`, an instance, or a name or an index, the
/// first instance of that applet (a fresh one to describe while none runs).
fn addressed(session: &mut Session, query: &str) -> anyhow::Result<Addressed> {
    let Some(position) = apps::instance_position(query) else {
        let entry = found(session, query)?;
        let target = Target::App(entry.index);
        return Ok(Addressed { entry, target });
    };
    let instances = session.instances()?;
    let instance = instances
        .iter()
        .find(|instance| instance.position == position)
        .with_context(|| format!("no instance @{position}; {} run", instances.len()))?;
    let entry = found(session, &instance.app.to_string())?;
    Ok(Addressed {
        entry,
        target: Target::Instance(position),
    })
}

/// The applet `query` names, and what it declares.
fn described(session: &mut Session, query: &str) -> anyhow::Result<(Addressed, AppDescription)> {
    let app = addressed(session, query)?;
    let description = session.describe(app.target)?;
    Ok((app, description))
}

/// One line for an instance: `* @1 RATCHET`, the one shown marked.
fn instance_line(session: &mut Session, instance: &InstanceEntry) -> anyhow::Result<String> {
    let entry = found(session, &instance.app.to_string())?;
    Ok(format!(
        "{} @{} {}",
        if instance.focused { '*' } else { ' ' },
        instance.position,
        entry.name
    ))
}

/// Routes the ports given of an instance, then prints where they all go.
fn route(session: &mut Session, query: &str, routes: &[String]) -> anyhow::Result<()> {
    let position = position(query)?;
    let description = session.describe(Target::Instance(position))?;
    let mut instance = session
        .instances()?
        .into_iter()
        .find(|instance| instance.position == position)
        .with_context(|| format!("no instance @{position}"))?;
    for route in routes {
        let (port, jack) = route
            .split_once('=')
            .with_context(|| format!("{route}: write it port=jack"))?;
        instance =
            match apps::find_port(&description, port).with_context(|| format!("no port {port}"))? {
                apps::PortRef::Output(index) => {
                    let output = apps::parse_output(jack).map_err(anyhow::Error::msg)?;
                    session.set_route(position, index, output)?
                }
                apps::PortRef::Input(index, trigger) => {
                    let jack = apps::parse_jack(jack, trigger).map_err(anyhow::Error::msg)?;
                    session.set_input(position, index, jack)?
                }
            };
    }
    print!("{}", apps::routing_text(&description, &instance));
    Ok(())
}

/// Prints a parameter's value, or an observable's, read now.
fn get(session: &mut Session, app: &str, name: &str) -> anyhow::Result<()> {
    let (app, description) = described(session, app)?;
    if let Some(spec) = apps::find_spec(&description.params, name) {
        let value = session.get_param(app.target, spec.index)?;
        println!("{}", apps::value_text(spec, value));
        return Ok(());
    }
    let spec = apps::find_spec(&description.observables, name)
        .with_context(|| format!("{} has no parameter or observable {name}", app.entry.name))?;
    match session.observe(app.target, spec.index)? {
        Some(value) => println!("{}", apps::value_text(spec, value)),
        None => println!("--"),
    }
    Ok(())
}

/// Sets a parameter and prints what the applet applied.
fn set(session: &mut Session, app: &str, param: &str, text: &str) -> anyhow::Result<()> {
    let (app, description) = described(session, app)?;
    let spec = apps::find_spec(&description.params, param)
        .with_context(|| format!("{} has no parameter {param}", app.entry.name))?;
    let value = apps::parse_value(spec, text).map_err(anyhow::Error::msg)?;
    let applied = session.set_param(app.target, spec.index, value)?;
    println!("{} {}", spec.label, apps::value_text(spec, applied));
    if applied != value {
        eprintln!("narrowed by another setting of {}", app.entry.name);
    }
    // A set may move other parameters, as the detent would (journal/D-043).
    let after = session.describe(app.target)?;
    for line in apps::moved(&description, &after, spec.index) {
        println!("{line}  (moved with it)");
    }
    Ok(())
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
    session.stream(1)?;
    let samples = collect(session, seconds * 1_000_000)?;
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
    session.stream(1)?;
    eprintln!("counting for {seconds} s of the module's clock");
    let samples = collect(session, seconds * 1_000_000)?;
    session.stream(0)?;
    let count = measure::count_edges(&samples);
    for (index, ((edges, intervals), high)) in count
        .rising
        .iter()
        .zip(&count.intervals)
        .zip(&count.high)
        .enumerate()
    {
        match intervals.mean() {
            Some(mean) => println!(
                "TR{}  {edges:>6} rising edges, {} to {} ms apart (mean {})",
                index + 1,
                millis(intervals.min),
                millis(intervals.max),
                millis(mean)
            ),
            None => println!("TR{}  {edges:>6} rising edges", index + 1),
        }
        if let Some(mean) = high.mean() {
            println!(
                "              high {} to {} ms (mean {}), to a tick",
                millis(high.min),
                millis(high.max),
                millis(mean)
            );
        }
    }
    if count.missed_ticks > 0 {
        println!(
            "INEXACT: {} of {} ticks had no sample (the host fell behind)",
            count.missed_ticks, count.ticks
        );
    }
    println!("{} ticks over {} ms", count.ticks, millis(count.micros));
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
            let _ = collect(session, args.settle_ms * 1_000)?;
            let settled = collect(session, args.average_ms * 1_000)?;
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

/// Microseconds as milliseconds, three decimals.
fn millis(micros: u64) -> String {
    format!("{}.{:03}", micros / 1_000, micros % 1_000)
}

/// Samples covering `micros` microseconds of the module's clock, from the
/// next one on.
fn collect(session: &mut Session, micros: u64) -> anyhow::Result<Vec<Sample>> {
    let mut samples: Vec<Sample> = Vec::new();
    loop {
        let sample = match session.next_sample() {
            Ok(sample) => sample,
            Err(ProbeError::Timeout) => continue,
            Err(error) => return Err(error.into()),
        };
        if let Some(first) = samples.first()
            && sample.micros.saturating_sub(first.micros) >= micros
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
