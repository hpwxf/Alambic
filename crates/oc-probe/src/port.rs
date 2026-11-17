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

/// Any line rate but 134, which restarts the module into its bootloader.
const BAUD: u32 = 115_200;

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
