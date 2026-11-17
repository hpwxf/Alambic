//! The WebAssembly core of the browser probe and flasher (journal/D-031).
//!
//! The page talks to the module through the browser's own APIs, which are
//! asynchronous: Web Serial for the probe port, `WebHID` for the bootloader.
//! This crate does everything else, without input or output of its own, so
//! that the code deciding what goes on the wire is the code the command-line
//! tools and the firmware already test:
//!
//! * the probe protocol: [`ProbeReader`] turns the bytes the port delivers
//!   into [`Message`]s, the `request_*` functions encode what to send
//!   (`oc-protocol`, rendered with `oc-probe`'s own text);
//! * the flash: [`Firmware`] runs the pre-flash gate of `cargo xtask flash`
//!   on an ELF and cuts the image into `HalfKay` reports (`oc-image`,
//!   `oc-halfkay`).
//!
//! Nothing here touches a `JsValue`, so the host tests link it as an
//! ordinary library.

use wasm_bindgen::prelude::wasm_bindgen;

use oc_core::framebuffer::LEN;
use oc_halfkay::Report;
use oc_image::flash::{FlashImage, flash_image};
use oc_probe::client::ProbeError;
use oc_probe::render;
use oc_protocol::{Error, FrameReader, MAX_FRAME, Reply, Request, VERSION, decode, encode};

/// The probe port's USB vendor identifier, for the Web Serial port filter.
#[wasm_bindgen]
#[must_use]
pub fn probe_vendor_id() -> u16 {
    oc_probe::VID
}

/// The probe port's USB product identifier.
#[wasm_bindgen]
#[must_use]
pub fn probe_product_id() -> u16 {
    oc_probe::PID
}

/// The bootloader's USB vendor identifier, for the `WebHID` device filter.
#[wasm_bindgen]
#[must_use]
pub fn loader_vendor_id() -> u16 {
    oc_halfkay::VENDOR_ID
}

/// The bootloader's USB product identifier.
#[wasm_bindgen]
#[must_use]
pub fn loader_product_id() -> u16 {
    oc_halfkay::PRODUCT_ID
}

/// One frame, ready to write to the port. Every request fits in
/// [`MAX_FRAME`], so encoding cannot fail.
fn frame(request: &Request) -> Vec<u8> {
    let mut buffer = [0; MAX_FRAME];
    encode(request, &mut buffer).map_or_else(|_| Vec::new(), <[u8]>::to_vec)
}

/// Opens a session; answered by a [`MessageKind::Hello`].
#[wasm_bindgen]
#[must_use]
pub fn request_hello() -> Vec<u8> {
    frame(&Request::Hello { protocol: VERSION })
}

/// Asks for the engine's profile.
#[wasm_bindgen]
#[must_use]
pub fn request_profile() -> Vec<u8> {
    frame(&Request::GetProfile)
}

/// Forgets the profile's durations.
#[wasm_bindgen]
#[must_use]
pub fn request_clear_profile() -> Vec<u8> {
    frame(&Request::ClearProfile)
}

/// Asks for a sample every `every_ticks` ticks; zero stops.
#[wasm_bindgen]
#[must_use]
pub fn request_stream(every_ticks: u16) -> Vec<u8> {
    frame(&Request::Stream { every_ticks })
}

/// Holds output `channel` (0 for A) at `millivolts`, or releases it.
#[wasm_bindgen]
#[must_use]
pub fn request_force_output(channel: u8, millivolts: Option<i32>) -> Vec<u8> {
    frame(&Request::ForceOutput {
        channel,
        millivolts,
    })
}

/// Asks for the screen, answered by eight [`MessageKind::ScreenPage`]s.
#[wasm_bindgen]
#[must_use]
pub fn request_screen() -> Vec<u8> {
    frame(&Request::GetScreen)
}

/// Restarts the module into its bootloader, ready to be flashed.
#[wasm_bindgen]
#[must_use]
pub fn request_reboot_to_loader() -> Vec<u8> {
    frame(&Request::RebootToLoader)
}

/// What a [`Message`] is.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// The answer to the hello; the text names the firmware and its build.
    Hello,
    /// The profile; the text is `oc-probe stats`'s table.
    Profile,
    /// One screen page: [`Message::page`] and [`Message::bytes`].
    ScreenPage,
    /// One sample of a stream; the text is `oc-probe stream`'s line.
    Sample,
    /// A request carried out, nothing else to say.
    Done,
    /// A refusal, or bytes that made no sense; the text says which.
    Error,
}

/// One message from the module, copied out of its frame.
#[wasm_bindgen]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    kind: MessageKind,
    text: String,
    page: u8,
    bytes: Vec<u8>,
}

#[wasm_bindgen]
impl Message {
    /// What it is.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn kind(&self) -> MessageKind {
        self.kind
    }

    /// What to show; empty for a screen page or a plain done.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn text(&self) -> String {
        self.text.clone()
    }

    /// A screen page's index, 0 at the top.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn page(&self) -> u8 {
        self.page
    }

    /// A screen page's column bytes.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

impl Message {
    fn with_text(kind: MessageKind, text: String) -> Self {
        Self {
            kind,
            text,
            page: 0,
            bytes: Vec::new(),
        }
    }

    fn refused(error: Error) -> Self {
        Self::with_text(MessageKind::Error, ProbeError::Refused(error).to_string())
    }

    fn from_reply(reply: Reply<'_>) -> Self {
        match reply {
            Reply::Hello { protocol, .. } if protocol != VERSION => {
                Self::refused(Error::Version(protocol))
            }
            Reply::Hello {
                firmware, build, ..
            } => Self::with_text(
                MessageKind::Hello,
                format!("firmware {firmware}, build {build}"),
            ),
            Reply::Profile(profile) => {
                Self::with_text(MessageKind::Profile, render::profile_table(&profile))
            }
            Reply::ScreenPage { page, bytes } => Self {
                kind: MessageKind::ScreenPage,
                text: String::new(),
                page,
                bytes: bytes.to_vec(),
            },
            Reply::Sample(sample) => {
                Self::with_text(MessageKind::Sample, render::sample_line(&sample))
            }
            Reply::Done => Self::with_text(MessageKind::Done, String::new()),
            Reply::Error(error) => Self::refused(error),
        }
    }
}

/// Splits what the probe port delivers into messages.
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct ProbeReader {
    reader: FrameReader,
}

#[wasm_bindgen]
impl ProbeReader {
    /// Nothing received yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the bytes of one read, which may end mid-frame; hands out every
    /// message they complete, in order.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Message> {
        let mut messages = Vec::new();
        for &byte in bytes {
            match self.reader.push(byte) {
                None => {}
                Some(Ok(frame)) => messages.push(match decode::<Reply<'_>>(frame) {
                    Ok(reply) => Message::from_reply(reply),
                    Err(_) => Message::with_text(
                        MessageKind::Error,
                        "a frame from the module did not decode".to_owned(),
                    ),
                }),
                Some(Err(error)) => messages.push(Message::refused(error)),
            }
        }
        messages
    }
}

/// The screen as RGBA pixels for a canvas, each pixel `scale` times wider
/// and taller, lit pixels white on black.
///
/// # Errors
///
/// When `screen` is not one whole framebuffer.
#[wasm_bindgen]
pub fn screen_rgba(screen: &[u8], scale: u32) -> Result<Vec<u8>, String> {
    let screen: &[u8; LEN] = screen
        .try_into()
        .map_err(|_| format!("a screen is {LEN} bytes, not {}", screen.len()))?;
    let (_, _, grey) = render::screen_grey(screen, scale);
    Ok(grey
        .into_iter()
        .flat_map(|level| [level, level, level, 0xFF])
        .collect())
}

/// A firmware image that passed the pre-flash gate, cut into the reports
/// that write it.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct Firmware {
    image: FlashImage,
    reports: Vec<Report>,
}

#[wasm_bindgen]
impl Firmware {
    /// Runs every check `cargo xtask flash` runs on `elf`, then lays it out
    /// as flash contents and cuts them into `HalfKay` reports.
    ///
    /// # Errors
    ///
    /// Every problem found, one per line.
    #[wasm_bindgen(constructor)]
    pub fn new(elf: &[u8]) -> Result<Firmware, String> {
        let image = flash_image(elf).map_err(|rejections| {
            rejections
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        let reports = oc_halfkay::reports(image.bytes())
            .map_err(|error| format!("cannot cut the image into loader reports: {error:?}"))?
            .collect();
        Ok(Self { image, reports })
    }

    /// The SHA-256 of the ELF, as `cargo xtask flash` prints it.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn sha256(&self) -> String {
        self.image.facts.sha256_hex()
    }

    /// Bytes the ELF's loadable segments hold.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn loadable_size(&self) -> u32 {
        u32::try_from(self.image.facts.loadable_size).unwrap_or(u32::MAX)
    }

    /// Bytes of flash written, from the start of the flash.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn flash_size(&self) -> u32 {
        u32::try_from(self.image.bytes().len()).unwrap_or(u32::MAX)
    }

    /// The entry point.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn entry(&self) -> u32 {
        self.image.facts.entry
    }

    /// Soft findings that did not stop the image.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        self.image.facts.warnings.clone()
    }

    /// Reports in all, the final boot request included.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn report_count(&self) -> usize {
        self.reports.len()
    }

    /// Report `index`'s bytes, to send as HID output report 0; empty past
    /// the end.
    #[must_use]
    pub fn report(&self, index: usize) -> Vec<u8> {
        self.reports
            .get(index)
            .map_or_else(Vec::new, |report| report.bytes().to_vec())
    }

    /// How long to keep retrying report `index` while the bootloader
    /// refuses it, in milliseconds.
    #[must_use]
    pub fn report_timeout_ms(&self, index: usize) -> u32 {
        self.reports.get(index).map_or(0, Report::timeout_ms)
    }
}

#[cfg(test)]
mod tests {
    use oc_protocol::{Profile, SCREEN_PAGES};

    use super::*;

    fn wire(reply: &Reply<'_>) -> Vec<u8> {
        let mut buffer = [0; MAX_FRAME];
        encode(reply, &mut buffer).unwrap().to_vec()
    }

    #[test]
    fn a_hello_round_trips_through_the_reader() {
        let mut reader = ProbeReader::new();
        let bytes = wire(&Reply::Hello {
            protocol: VERSION,
            firmware: "0.4.0",
            build: "abc1234",
        });
        let messages = reader.push(&bytes);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].kind(), MessageKind::Hello);
        assert_eq!(messages[0].text, "firmware 0.4.0, build abc1234");
    }

    #[test]
    fn another_protocol_version_is_an_error() {
        let mut reader = ProbeReader::new();
        let messages = reader.push(&wire(&Reply::Hello {
            protocol: VERSION + 1,
            firmware: "9",
            build: "x",
        }));
        assert_eq!(messages[0].kind(), MessageKind::Error);
        assert!(messages[0].text.contains("flash a matching firmware"));
    }

    #[test]
    fn frames_split_across_reads_come_out_whole_and_in_order() {
        let mut bytes = wire(&Reply::Done);
        bytes.extend(wire(&Reply::ScreenPage {
            page: 3,
            bytes: &[0xAA; 128],
        }));
        bytes.extend(wire(&Reply::Profile(Profile::new(600))));
        let mut reader = ProbeReader::new();
        let mut messages = Vec::new();
        for chunk in bytes.chunks(7) {
            messages.extend(reader.push(chunk));
        }
        let kinds: Vec<_> = messages.iter().map(Message::kind).collect();
        assert_eq!(
            kinds,
            [
                MessageKind::Done,
                MessageKind::ScreenPage,
                MessageKind::Profile
            ]
        );
        assert_eq!(messages[1].page(), 3);
        assert_eq!(messages[1].bytes(), [0xAA; 128]);
    }

    #[test]
    fn garbage_is_reported_not_fatal() {
        let mut reader = ProbeReader::new();
        let mut bytes = vec![0x05, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
        bytes.extend(wire(&Reply::Done));
        let messages = reader.push(&bytes);
        assert_eq!(messages[0].kind(), MessageKind::Error);
        assert_eq!(messages[1].kind(), MessageKind::Done);
    }

    #[test]
    fn requests_are_single_frames() {
        for request in [
            request_hello(),
            request_profile(),
            request_clear_profile(),
            request_stream(10),
            request_force_output(2, Some(-3_000)),
            request_force_output(2, None),
            request_screen(),
            request_reboot_to_loader(),
        ] {
            assert_eq!(request.last(), Some(&0));
            assert!(!request[..request.len() - 1].contains(&0));
        }
    }

    #[test]
    fn the_screen_becomes_four_bytes_a_pixel() {
        let mut screen = [0; LEN];
        screen[0] = 0x01; // the top-left pixel
        let rgba = screen_rgba(&screen, 2).unwrap();
        let pages = usize::from(SCREEN_PAGES);
        assert_eq!(rgba.len(), LEN * pages * 4 * 4);
        assert_eq!(
            &rgba[..8],
            &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(&rgba[8..12], &[0, 0, 0, 0xFF]);
        assert!(screen_rgba(&[0; 10], 1).is_err());
    }

    #[test]
    fn an_image_that_fails_the_gate_is_refused_with_every_reason() {
        let error = Firmware::new(b":020000040000FA\n").unwrap_err();
        assert!(error.contains("not an ELF"), "{error}");
    }
}
