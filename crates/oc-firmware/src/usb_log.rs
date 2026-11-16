//! The `log` facade's backend: whole lines into a queue, sent on the first
//! USB serial port by [`crate::usb::Usb`] (journal/D-011, D-029).
//!
//! Lines keep the format the previous backend (`imxrt-log`) used,
//! `[LEVEL target]: message`, so serial monitors and `TESTING.md` read them
//! the same. A line that does not fit in the queue is dropped whole.

use core::cell::RefCell;
use core::fmt::Write as _;

use cortex_m::interrupt::{self, Mutex};

use oc_core::fmt::TextBuf;
use oc_protocol::queue::ByteQueue;

/// Bytes of log waiting for the host: a few seconds of heartbeat lines, so
/// a monitor that connects late still sees the last ones.
pub(crate) const LOG_BYTES: usize = 4_096;

/// Longest line; longer ones are cut.
const LINE_BYTES: usize = 320;

static QUEUE: Mutex<RefCell<ByteQueue<LOG_BYTES>>> = Mutex::new(RefCell::new(ByteQueue::new()));

static LOGGER: Logger = Logger;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut line = TextBuf::<LINE_BYTES>::new();
        let _ = write!(
            line,
            "[{} {}]: {}\r\n",
            record.level(),
            record.target(),
            record.args()
        );
        with_queue(|queue| {
            queue.push(line.as_str().as_bytes());
        });
    }

    fn flush(&self) {}
}

/// Installs the backend. A second call changes nothing.
pub(crate) fn init() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// Runs `f` on the queue, with interrupts off: the panic handler logs too.
pub(crate) fn with_queue<R>(f: impl FnOnce(&mut ByteQueue<LOG_BYTES>) -> R) -> R {
    interrupt::free(|cs| f(&mut QUEUE.borrow(cs).borrow_mut()))
}
