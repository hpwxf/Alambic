//! The inputs of an applet's assignable parameters and the submenu that
//! edits one of them (journal/D-045): the state every applet taking CV
//! inputs holds the same way, so that the panel, the host probe and the
//! applet's own rendering all reach it alike.
//!
//! A parameter's slot is its rank among the [`ParamSpec::assignable`]
//! parameters of its applet ([`slot_of`]): the parameter's index alone
//! names its input, without a table of its own in each applet.

use super::editor::{AssignEditor, Param};
use super::{Assign, AssignLine, Effect, Source};
use crate::apps::InputSnapshot;
use crate::apps::param::ParamSpec;
use crate::framebuffer::FrameBuffer;
use crate::platform::MilliVolts;
use crate::text::TextStyle;

/// The slot of parameter `index` among `specs`: its rank among the
/// assignable ones, `None` for one that takes no input or past the last.
#[must_use]
pub fn slot_of(specs: &[ParamSpec], index: usize) -> Option<usize> {
    let spec = specs.get(index)?;
    spec.assignable
        .then(|| specs[..index].iter().filter(|spec| spec.assignable).count())
}

/// The input of each of `N` assignable parameters, what each read last,
/// and the submenu while it is open on one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modulations<const N: usize> {
    assigns: [Assign; N],
    /// The slot the submenu is open on, and the submenu.
    editor: Option<(usize, AssignEditor)>,
    /// What each slot's input read on the last tick, unshaped: a jack, or a
    /// port of another instance (journal/D-048).
    raw: [MilliVolts; N],
    /// The name of the port each slot's input reads, when it reads one
    /// that is there: what the screen and the submenu write after its slot
    /// (`+1:NOT`, `1:NOTE`).
    port_names: [Option<&'static str>; N],
}

impl<const N: usize> Modulations<N> {
    /// `assigns` in their slots, the submenu closed, the inputs at 0 V.
    #[must_use]
    pub const fn new(assigns: [Assign; N]) -> Self {
        Self {
            assigns,
            editor: None,
            raw: [0; N],
            port_names: [None; N],
        }
    }

    /// Opens the submenu on `slot`; past the last, it stays closed.
    pub fn open(&mut self, slot: usize) {
        if slot < N {
            self.editor = Some((slot, AssignEditor::new()));
        }
    }

    /// Whether the submenu is open, the panel then its own.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.editor.is_some()
    }

    /// Follows the panel while the submenu is open, `sweep` the signal that
    /// sweeps its parameter end to end when it follows its input (`None`, a
    /// parameter without one, closes it); returns whether it was open, the
    /// panel's turns and clicks then spent.
    pub fn follow(
        &mut self,
        input: &InputSnapshot,
        sweep: Option<(MilliVolts, MilliVolts)>,
    ) -> bool {
        let Some((slot, editor)) = &mut self.editor else {
            return false;
        };
        let open = sweep.is_some_and(|sweep| editor.follow(&mut self.assigns[*slot], input, sweep));
        if !open {
            self.editor = None;
        }
        true
    }

    /// Reads each slot's input off `input`, a jack or another instance's
    /// port on its bus, and takes the open submenu's into the range it has
    /// seen, `input.elapsed_micros` after the last.
    pub fn read(&mut self, input: &InputSnapshot) {
        for ((raw, name), assign) in self
            .raw
            .iter_mut()
            .zip(&mut self.port_names)
            .zip(&self.assigns)
        {
            *raw = assign.raw(input).unwrap_or(0);
            *name = match assign.input {
                Some(Source::Lane { slot, port }) => input
                    .bus
                    .port(usize::from(slot), usize::from(port))
                    .map(|port| port.name),
                _ => None,
            };
        }
        if let Some((slot, editor)) = &mut self.editor
            && self.assigns[*slot].input.is_some()
        {
            editor.observe(self.raw[*slot], input.elapsed_micros);
        }
    }

    /// The input of `slot`.
    ///
    /// # Panics
    ///
    /// Past the last slot, which only an applet's own mistake can name.
    #[must_use]
    pub const fn assign(&self, slot: usize) -> &Assign {
        &self.assigns[slot]
    }

    /// The shaped signal of `slot`'s input and its effect, `None` without an
    /// input.
    #[must_use]
    pub fn reading(&self, slot: usize) -> Option<(MilliVolts, Effect)> {
        self.assigns[slot].reading(self.raw[slot])
    }

    /// The name of the port `slot`'s input reads, `None` for a jack, no
    /// input, or a port that is no more.
    #[must_use]
    pub const fn port_name(&self, slot: usize) -> Option<&'static str> {
        self.port_names[slot]
    }

    /// Draws at the right of a setting's row, from `top` up to `right`,
    /// `slot`'s input (see [`super::draw_input`]).
    pub fn draw_input(
        &self,
        frame: &mut FrameBuffer,
        slot: usize,
        right: i32,
        top: i32,
        style: TextStyle,
    ) {
        super::draw_input(
            frame,
            &self.assigns[slot],
            self.port_names[slot],
            right,
            top,
            style,
        );
    }

    /// The field picked in the submenu, `None` while it is closed.
    #[must_use]
    pub const fn line(&self) -> Option<AssignLine> {
        match &self.editor {
            Some((_, editor)) => Some(editor.line()),
            None => None,
        }
    }

    /// The lowest and highest reading of the submenu's input lately (see
    /// [`AssignEditor::range`]), `None` while it is closed or before the
    /// first.
    #[must_use]
    pub fn seen(&self) -> Option<(MilliVolts, MilliVolts)> {
        self.editor.as_ref().and_then(|(_, editor)| editor.range())
    }

    /// Draws the submenu over what the frame holds, for `param`; nothing
    /// while it is closed.
    pub fn render(&self, frame: &mut FrameBuffer, param: &Param<'_>) {
        if let Some((slot, editor)) = &self.editor {
            editor.render(
                frame,
                &self.assigns[*slot],
                self.raw[*slot],
                self.port_names[*slot],
                param,
            );
        }
    }

    /// Every slot's input, for [`App::assigns`](crate::apps::App::assigns).
    #[must_use]
    pub const fn as_slice(&self) -> &[Assign] {
        &self.assigns
    }

    /// Every slot's input, for
    /// [`App::assigns_mut`](crate::apps::App::assigns_mut).
    pub const fn as_mut_slice(&mut self) -> &mut [Assign] {
        &mut self.assigns
    }
}

#[cfg(test)]
mod tests {
    use super::slot_of;
    use crate::apps::param::{ParamSpec, Unit};

    #[test]
    fn a_slot_is_the_rank_among_the_assignable_parameters() {
        let number = ParamSpec::number("N", (0, 9), 1, 0, Unit::None);
        let specs = [
            number.assignable(),
            ParamSpec::toggle("T"),
            number.assignable(),
            number,
            number.assignable(),
        ];
        assert_eq!(slot_of(&specs, 0), Some(0));
        assert_eq!(slot_of(&specs, 1), None, "takes no input");
        assert_eq!(slot_of(&specs, 2), Some(1));
        assert_eq!(slot_of(&specs, 3), None);
        assert_eq!(slot_of(&specs, 4), Some(2));
        assert_eq!(slot_of(&specs, 5), None, "past the last");
    }
}
