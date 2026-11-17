//! The Teensy's USB device: two serial ports, polled from the loop
//! (journal/D-011, D-029).
//!
//! The first port carries the text log ([`crate::usb_log`]), readable with
//! any serial monitor; the second carries the probe protocol
//! (`oc_protocol`), for `oc-probe`. One composite device, two CDC ACM
//! functions tied to their interfaces by association descriptors, which
//! macOS, Linux and Windows show as two serial ports. Interrupts stay off:
//! everything happens in [`Usb::poll`], as with the previous backend.
//!
//! Setting either port to 134 baud asks for a restart into the bootloader,
//! the gesture Teensyduino's tools use. No serial data is involved: a
//! virtual port has no line rate, but CDC still lets the host set one, and
//! `stty <port> 134` (or any `tcsetattr`) becomes a `SET_LINE_CODING` class
//! request on endpoint 0, seven bytes whose first four carry the rate. The
//! class keeps the last value; [`Usb::poll`] reads it back after each poll.
//! 134 (once 134.5 baud, an IBM 2741 rate) is a value no ordinary program
//! sets, so a terminal cannot restart the module by accident. What happens
//! next is in [`crate::reboot`].

use static_cell::StaticCell;
use teensy4_bsp::usbd::{BusAdapter, EndpointMemory, EndpointState, Instances};
use usb_device::UsbDirection;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{
    StringDescriptors, UsbDevice, UsbDeviceBuilder, UsbDeviceState, UsbVidPid,
};
use usb_device::endpoint::EndpointAddress;
use usbd_serial::CdcAcmClass;

use oc_core::Engine;
use oc_core::platform::{AnalogIn, AnalogOut, Clock, Controls, DigitalIn, Display};
use oc_protocol::queue::ByteQueue;
use oc_protocol::server::Server;

/// The identifiers the previous backend used: the pid.codes test pair.
const VID_PID: UsbVidPid = UsbVidPid(0x5824, 0x27dd);

/// High-speed bulk packets.
const MAX_PACKET: u16 = 512;

/// [`MAX_PACKET`] as a buffer length.
const PACKET_BYTES: usize = MAX_PACKET as usize;

/// Control packets on endpoint 0.
const EP0_PACKET: u8 = 64;

/// The line rate that asks for the bootloader (Teensyduino's convention).
const REBOOT_BAUD: u32 = 134;

/// Room for every endpoint's packets: two ports of two bulk endpoints and a
/// small interrupt one, plus endpoint 0, with margin.
static ENDPOINT_MEMORY: EndpointMemory<4_096> = EndpointMemory::new();
static ENDPOINT_STATE: EndpointState = EndpointState::max_endpoints();
static BUS: StaticCell<UsbBusAllocator<BusAdapter>> = StaticCell::new();

type Port = CdcAcmClass<'static, BusAdapter>;

/// The device and its two ports.
pub(crate) struct Usb {
    device: UsbDevice<'static, BusAdapter>,
    log: Port,
    probe: Port,
    configured: bool,
    reboot_requested: bool,
}

impl Usb {
    /// Brings the device up; the host enumerates it on the following polls.
    pub(crate) fn new(instances: Instances<1>) -> Self {
        let bus = BusAdapter::new(instances, &ENDPOINT_MEMORY, &ENDPOINT_STATE);
        bus.set_interrupts(false);
        let bus = BUS.init(UsbBusAllocator::new(bus));
        let log = CdcAcmClass::new_with_interface_names(
            bus,
            MAX_PACKET,
            Some("Alambic log"),
            Some("Alambic log data"),
        );
        let probe = CdcAcmClass::new_with_interface_names(
            bus,
            MAX_PACKET,
            Some("Alambic probe"),
            Some("Alambic probe data"),
        );
        let device = UsbDeviceBuilder::new(bus, VID_PID)
            .composite_with_iads()
            .strings(&[StringDescriptors::default()
                .manufacturer("Alambic")
                .product("Ornament & Crime (Alambic)")])
            .unwrap_or_else(|_| panic!("USB strings"))
            .max_packet_size_0(EP0_PACKET)
            .unwrap_or_else(|_| panic!("USB endpoint 0 size"))
            .build();
        // CDC needs a zero-length packet after a full one; the controller
        // sends it when asked, on every endpoint the classes may have taken.
        for index in 1..8 {
            for direction in [UsbDirection::In, UsbDirection::Out] {
                device
                    .bus()
                    .enable_zlt(EndpointAddress::from_parts(index, direction));
            }
        }
        Self {
            device,
            log,
            probe,
            configured: false,
            reboot_requested: false,
        }
    }

    /// Serves the bus and sends what the log has queued. Cheap when no host
    /// is there.
    pub(crate) fn poll(&mut self) {
        self.device.poll(&mut [&mut self.log, &mut self.probe]);
        if self.device.state() != UsbDeviceState::Configured {
            self.configured = false;
            return;
        }
        if !self.configured {
            // The controller must be told once it is configured.
            self.device.bus().configure();
            self.configured = true;
        }
        for port in [&self.log, &self.probe] {
            if port.line_coding().data_rate() == REBOOT_BAUD {
                self.reboot_requested = true;
            }
        }
        // Nothing is read from the log port, but what the host sends must
        // be taken, or it is refused forever.
        let mut ignored = [0; PACKET_BYTES];
        let _ = self.log.read_packet(&mut ignored);
        crate::usb_log::with_queue(|queue| send(&mut self.log, queue));
    }

    /// Exchanges the probe's bytes: requests in, replies and samples out.
    /// A host that closes the port (drops DTR) ends the session.
    pub(crate) fn serve<A, O, D, C, K, S>(
        &mut self,
        server: &mut Server,
        engine: &mut Engine<A, O, D, C, K, S>,
    ) where
        A: AnalogIn,
        O: AnalogOut,
        D: DigitalIn,
        C: Controls,
        K: Clock,
        S: Display,
    {
        if !self.configured || !self.probe.dtr() {
            if server.in_session() {
                server.close(engine);
            }
            return;
        }
        let mut received = [0; PACKET_BYTES];
        if let Ok(count) = self.probe.read_packet(&mut received) {
            server.receive(&received[..count], engine);
        }
        send(&mut self.probe, server.outgoing());
    }

    /// Whether a host set a port to 134 baud; true once.
    pub(crate) const fn take_reboot_request(&mut self) -> bool {
        core::mem::replace(&mut self.reboot_requested, false)
    }
}

/// Writes one packet of what `queue` holds, if the port takes it.
fn send<const N: usize>(port: &mut Port, queue: &mut ByteQueue<N>) {
    let waiting = queue.peek(PACKET_BYTES);
    if waiting.is_empty() {
        return;
    }
    if let Ok(written) = port.write_packet(waiting) {
        queue.consume(written);
    }
}
