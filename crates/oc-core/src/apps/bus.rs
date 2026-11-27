//! What the instances publish to one another (journal/D-048): the level each
//! output port carried on the last tick, whether or not its route sends it
//! to a jack, for an assignment of another instance to read as it would a
//! CV input, without the converters.
//!
//! The last tick's, for every slot alike: a reader sees the same thing
//! whatever its place left or right of the port it reads, one tick (1 ms)
//! late, and no loop can arise. Only a port that carries a level
//! ([`PortSignal::Cv`]) is offered; an audio port, a port past the last, or
//! the reader's own reads 0 V.

use super::{MAX_INSTANCES, Port, PortSignal};
use crate::modulation::Source;
use crate::platform::{CV_CHANNELS, MilliVolts};

/// The levels and the ports of every slot, as one instance sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bus {
    /// The level each lane of each slot published on the last tick.
    levels: [[MilliVolts; CV_CHANNELS]; MAX_INSTANCES],
    /// The output ports of each slot's instance.
    ports: [&'static [Port]; MAX_INSTANCES],
    /// The slot of the instance reading it, whose own ports it never offers.
    own: Option<usize>,
}

impl Bus {
    /// Nothing published, no port: what an applet run outside a host sees.
    pub const EMPTY: Self = Self {
        levels: [[0; CV_CHANNELS]; MAX_INSTANCES],
        ports: [&[]; MAX_INSTANCES],
        own: None,
    };

    /// `levels` on the `ports` of each slot, as the instance in slot `own`
    /// (`None`: none of them) reads them.
    #[must_use]
    pub const fn new(
        levels: [[MilliVolts; CV_CHANNELS]; MAX_INSTANCES],
        ports: [&'static [Port]; MAX_INSTANCES],
        own: Option<usize>,
    ) -> Self {
        Self { levels, ports, own }
    }

    /// Whether port `port` of slot `slot` can be read: it carries a level
    /// and belongs to another instance than the reader.
    #[must_use]
    pub fn offers(&self, slot: usize, port: usize) -> bool {
        self.own != Some(slot)
            && self
                .port(slot, port)
                .is_some_and(|port| port.signal == PortSignal::Cv)
    }

    /// The level port `port` of slot `slot` published on the last tick, 0 V
    /// for one it does not offer.
    #[must_use]
    pub fn level(&self, slot: usize, port: usize) -> MilliVolts {
        if self.offers(slot, port) {
            self.levels[slot][port]
        } else {
            0
        }
    }

    /// Port `port` of slot `slot`, offered or not, if there is one.
    #[must_use]
    pub fn port(&self, slot: usize, port: usize) -> Option<Port> {
        self.ports.get(slot)?.get(port).copied()
    }

    /// Every port it offers, slot by slot, in their order.
    pub fn sources(&self) -> impl Iterator<Item = Source> + '_ {
        (0..MAX_INSTANCES).flat_map(move |slot| {
            (0..CV_CHANNELS)
                .filter(move |&port| self.offers(slot, port))
                .filter_map(move |port| Source::lane(slot, port))
        })
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::Bus;
    use crate::apps::{MAX_INSTANCES, PORTS_A_TO_D, Port};
    use crate::modulation::Source;

    const AUDIO: [Port; 1] = [Port::audio("OUT")];

    fn bus() -> Bus {
        let mut levels = [[0; 4]; MAX_INSTANCES];
        levels[0] = [100, 200, 300, 400];
        levels[1] = [9, 9, 9, 9];
        levels[2] = [5, 6, 7, 8];
        Bus::new(
            levels,
            [&PORTS_A_TO_D, &AUDIO, &PORTS_A_TO_D[..2], &[]],
            Some(2),
        )
    }

    #[test]
    fn a_cv_port_of_another_instance_reads_its_level() {
        assert_eq!(bus().level(0, 2), 300);
    }

    #[test]
    fn an_audio_port_the_own_slot_and_a_missing_port_read_nothing() {
        let bus = bus();
        assert_eq!(bus.level(1, 0), 0, "audio");
        assert_eq!(bus.level(2, 0), 0, "the reader's own");
        assert_eq!(bus.level(0, 9), 0, "past the last port");
        assert_eq!(bus.level(3, 0), 0, "an empty slot");
        assert_eq!(Bus::EMPTY.level(0, 0), 0);
    }

    #[test]
    fn the_sources_are_the_other_instances_cv_ports_in_order() {
        let expected = (0..4).filter_map(|port| Source::lane(0, port));
        assert!(
            bus().sources().eq(expected),
            "slot 0's four ports; slot 1 audio, slot 2 the reader's"
        );
    }
}
