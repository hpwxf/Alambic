//! The engine's profile as two USB log lines, once a second (journal/D-029):
//! enough to read the cost of a frame and the loop jitter before the host
//! tool exists, and from a plain serial monitor afterwards.

use core::fmt::Write as _;

use oc_core::fmt::TextBuf;
use oc_core::profile::{Profile, Stage, Stat};

/// Longest profile line: nine stages of up to two seven-digit durations.
/// Fits the log backend's own line limit.
const LINE_BYTES: usize = 256;

/// Logs the mean and longest duration of every stage that ran, then the
/// period's extremes and its jitter histogram, all since the splash ended.
pub(crate) fn log(profile: &Profile) {
    let mut line = TextBuf::<LINE_BYTES>::new();
    let _ = line.write_str("profile us mean/max:");
    for stage in Stage::ALL {
        let stat = profile.stage(stage);
        if let (Some(mean), Some(max)) = (stat.mean(), stat.max()) {
            let _ = write!(line, " {}=", stage.name());
            write_micros(&mut line, profile, mean);
            let _ = line.write_char('/');
            write_micros(&mut line, profile, max);
        }
    }
    log::info!("{}", line.as_str());

    let mut line = TextBuf::<LINE_BYTES>::new();
    let period: &Stat = profile.period();
    if let (Some(min), Some(max)) = (period.min(), period.max()) {
        let _ = line.write_str("period us ");
        write_micros(&mut line, profile, min);
        let _ = line.write_str("..");
        write_micros(&mut line, profile, max);
        let _ = write!(line, " n={} jitter(<1,<2,<4..)=", period.count());
        for (index, count) in profile.jitter().iter().enumerate() {
            let separator = if index == 0 { "" } else { "," };
            let _ = write!(line, "{separator}{count}");
        }
        log::info!("{}", line.as_str());
    }
}

/// Writes `fine` counts as microseconds with one decimal.
fn write_micros(line: &mut TextBuf<LINE_BYTES>, profile: &Profile, fine: u32) {
    let nanos = profile.nanos(fine);
    let _ = write!(line, "{}.{}", nanos / 1_000, nanos % 1_000 / 100);
}
