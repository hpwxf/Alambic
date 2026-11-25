//! Finding the module's probe port among the host's serial ports.
//!
//! The module shows two ports with the same USB identifiers; only the probe
//! port answers a hello, so each candidate is tried in turn. Where the host
//! cannot tell which ports are USB (Linux without libudev), every port is a
//! candidate.

use std::time::Duration;

use serialport::{SerialPort, SerialPortType};

use crate::client::{Client, ModuleInfo, ProbeError};

pub use crate::{PID, VID};

/// How long a read waits before giving up.
pub const TIMEOUT: Duration = Duration::from_millis(500);

/// Any line rate but [`RESTART_BAUD`].
const BAUD: u32 = 115_200;

/// The line rate that restarts the module into its bootloader, on either of
/// its ports: Teensyduino's gesture, which every firmware of this project
/// has honoured since it had USB ports (lot 3b3), whatever its probe
/// protocol. A virtual port has no rate; setting one is a USB request the
/// firmware reads (`oc-firmware`'s `usb.rs`).
pub const RESTART_BAUD: u32 = 134;

/// An open probe session.
pub type Session = Client<Box<dyn SerialPort>>;

/// Opens `path` and says hello.
///
/// # Errors
///
/// When the port cannot be opened or the module does not answer.
pub fn open(path: &str) -> anyhow::Result<(Session, ModuleInfo)> {
    let mut port = serialport::new(path, BAUD).timeout(TIMEOUT).open()?;
    // The module serves the probe only while the host holds DTR.
    port.write_data_terminal_ready(true)?;
    let mut client = Client::new(port);
    let info = client.hello()?;
    Ok((client, info))
}

/// Asks the module to restart into its bootloader by setting `path`, or
/// each port that may be the module's, to [`RESTART_BAUD`]: no session, so
/// a firmware speaking another protocol restarts too. The ports set; empty
/// when none could be opened (the module is unplugged, already in its
/// bootloader, or its ports are held elsewhere).
///
/// # Errors
///
/// When the host cannot list its ports.
pub fn restart_into_loader(path: Option<&str>) -> anyhow::Result<Vec<String>> {
    let paths = match path {
        Some(path) => vec![path.to_owned()],
        None => candidates()?,
    };
    let mut set = Vec::new();
    for path in paths {
        // Opening at that rate is the request; the port goes away with the
        // module, so the second of its two may fail to open: one is enough.
        if serialport::new(&path, RESTART_BAUD)
            .timeout(TIMEOUT)
            .open()
            .is_ok()
        {
            set.push(path);
        }
    }
    Ok(set)
}

/// The serial ports that may be the module's, the callout (`cu.`) side
/// only on macOS.
///
/// # Errors
///
/// When the host cannot list its ports.
pub fn candidates() -> anyhow::Result<Vec<String>> {
    let mut paths = Vec::new();
    for port in serialport::available_ports()? {
        if port.port_name.starts_with("/dev/tty.") {
            continue;
        }
        let ours = match &port.port_type {
            SerialPortType::UsbPort(usb) => usb.vid == VID && usb.pid == PID,
            SerialPortType::Unknown => true,
            SerialPortType::PciPort | SerialPortType::BluetoothPort => false,
        };
        if ours {
            paths.push(port.port_name);
        }
    }
    Ok(paths)
}

/// Opens `path` if given, else the first candidate that answers.
///
/// # Errors
///
/// When no port answers, with what each one said.
pub fn find(path: Option<&str>) -> anyhow::Result<(String, Session, ModuleInfo)> {
    if let Some(path) = path {
        let (session, info) = open(path)?;
        return Ok((path.to_owned(), session, info));
    }
    let mut tried = Vec::new();
    for path in candidates()? {
        match open(&path) {
            Ok((session, info)) => return Ok((path, session, info)),
            Err(error) => {
                // A version mismatch is the answer, not a reason to go on.
                if let Some(ProbeError::Refused(_)) = error.downcast_ref::<ProbeError>() {
                    return Err(error.context(path));
                }
                tried.push(format!("{path}: {error}"));
            }
        }
    }
    if tried.is_empty() {
        anyhow::bail!("no serial port looks like the module: is it plugged in and running?");
    }
    anyhow::bail!(
        "no port answered as the module's probe:\n  {}",
        tried.join("\n  ")
    )
}
