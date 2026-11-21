//! The applets in the browser (journal/D-043): the page hands each message
//! of a `ListApps` or a `DescribeApp` answer to an [`AppList`] or an
//! [`AppView`], then reads names, values and editors from it, and asks it
//! for the next request. Finding, parsing and writing a value is
//! `oc-probe`'s own code, the command line's, so the page shows and accepts
//! what `cargo probe` does.

use oc_probe::apps::{edit_text, parse_value, value_text};
use oc_probe::client::{AppDescription, AppEntry, SpecEntry};
use oc_protocol::{Group, ParamKind, ParamValue, Request};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{Message, MessageKind, frame};

/// What an applet message carried, kept beside its text for the views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Applet {
    /// One applet of a list.
    App(AppEntry),
    /// One parameter or observable of a description.
    Spec(Group, SpecEntry),
    /// One name of the choice before it.
    ChoiceName {
        group: Group,
        index: u8,
        name: String,
    },
    /// One action of a description.
    Action(String),
    /// A parameter's value, read or applied.
    Value(ParamValue),
    /// An observable's value, `None` while unknown.
    Observed(Option<ParamValue>),
}

/// How the page edits a parameter.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editor {
    /// A text field: a number, or a note's name.
    Number,
    /// A list of names.
    Choice,
    /// On or off.
    Toggle,
}

/// The applets the module runs, from a `ListApps` answer.
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct AppList {
    apps: Vec<AppEntry>,
}

#[wasm_bindgen]
impl AppList {
    /// Nothing listed yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one message of the answer; true once it is complete.
    pub fn absorb(&mut self, message: &Message) -> bool {
        if let Some(Applet::App(app)) = &message.applet {
            self.apps.push(app.clone());
        }
        message.kind == MessageKind::Done
    }

    /// How many applets.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    /// Whether none was listed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// Applet `at`'s index, which names it to the module.
    #[must_use]
    pub fn index(&self, at: usize) -> u8 {
        self.apps.get(at).map_or(u8::MAX, |app| app.index)
    }

    /// Applet `at`'s name.
    #[must_use]
    pub fn name(&self, at: usize) -> String {
        self.apps
            .get(at)
            .map_or_else(String::new, |app| app.name.clone())
    }

    /// Whether applet `at` drives the outputs and the screen.
    #[must_use]
    pub fn running(&self, at: usize) -> bool {
        self.apps.get(at).is_some_and(|app| app.running)
    }
}

/// One applet's parameters, observables and actions, from a `DescribeApp`
/// answer, kept up to date by the values that come back.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct AppView {
    app: u8,
    description: AppDescription,
    broken: bool,
    /// Which parameters the last refresh found changed.
    moved: Vec<bool>,
}

#[wasm_bindgen]
impl AppView {
    /// Applet `app`, nothing described yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(app: u8) -> Self {
        Self {
            app,
            description: AppDescription::default(),
            broken: false,
            moved: Vec::new(),
        }
    }

    /// The request that describes it, answered by the messages
    /// [`Self::absorb`] takes.
    #[must_use]
    pub fn describe_request(&self) -> Vec<u8> {
        frame(&Request::DescribeApp { app: self.app })
    }

    /// Takes one message of the description; true once it is complete.
    pub fn absorb(&mut self, message: &Message) -> bool {
        match &message.applet {
            Some(Applet::Spec(group, spec)) => self.description.add_spec(*group, spec.clone()),
            Some(Applet::ChoiceName { group, index, name }) => {
                if self
                    .description
                    .add_choice_name(*group, *index, name.clone())
                    .is_err()
                {
                    self.broken = true;
                }
            }
            Some(Applet::Action(name)) => self.description.actions.push(name.clone()),
            _ => {}
        }
        message.kind == MessageKind::Done
    }

    /// The request that reads every value again, after a set that may have
    /// moved other parameters with it (journal/D-043): answered by the
    /// messages [`Self::absorb_refresh`] takes.
    #[must_use]
    pub fn refresh_request(&self) -> Vec<u8> {
        self.describe_request()
    }

    /// Takes one message of a refresh: the values it carries replace the
    /// ones kept, and [`Self::param_moved`] says which changed; true once it
    /// is complete.
    pub fn absorb_refresh(&mut self, message: &Message) -> bool {
        if self.moved.len() != self.description.params.len() {
            self.moved = vec![false; self.description.params.len()];
        }
        if let Some(Applet::Spec(group, fresh)) = &message.applet {
            let list = match group {
                Group::Param => &mut self.description.params,
                Group::Observable => &mut self.description.observables,
            };
            if let Some(at) = list.iter().position(|spec| spec.index == fresh.index) {
                if *group == Group::Param && list[at].value != fresh.value {
                    self.moved[at] = true;
                }
                list[at].value = fresh.value;
                list[at].label.clone_from(&fresh.label);
                list[at].active = fresh.active;
            }
        }
        message.kind == MessageKind::Done
    }

    /// Whether parameter `at` changed in the last refresh; cleared by
    /// [`Self::settle`].
    #[must_use]
    pub fn param_moved(&self, at: usize) -> bool {
        self.moved.get(at).copied().unwrap_or(false)
    }

    /// Forgets which parameters the last refresh found changed, before the
    /// next set.
    pub fn settle(&mut self) {
        self.moved.clear();
    }

    /// Whether the description came out of order, and cannot be trusted.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn broken(&self) -> bool {
        self.broken
    }

    /// How many parameters.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn param_count(&self) -> usize {
        self.description.params.len()
    }

    /// Parameter `at`'s name, which finds it whatever its label.
    #[must_use]
    pub fn param_name(&self, at: usize) -> String {
        param(&self.description, at).map_or_else(String::new, |spec| spec.name.clone())
    }

    /// How the module's screen labels parameter `at` now (`SPAN` for
    /// `RATCHET`'s `FIRST` in `MODE SPAN`); a refresh keeps it current.
    #[must_use]
    pub fn param_label(&self, at: usize) -> String {
        param(&self.description, at).map_or_else(String::new, |spec| spec.label.clone())
    }

    /// Parameter `at`'s value as the module writes it (`-3.0 OCT`).
    #[must_use]
    pub fn param_text(&self, at: usize) -> String {
        param(&self.description, at).map_or_else(String::new, shown)
    }

    /// Parameter `at`'s value as one types it back (`-3.0`).
    #[must_use]
    pub fn param_edit_text(&self, at: usize) -> String {
        param(&self.description, at)
            .and_then(|spec| spec.value.map(|value| edit_text(spec, value)))
            .unwrap_or_default()
    }

    /// The values parameter `at` takes, as text (`1..32`, `FIRST|SPAN`).
    #[must_use]
    pub fn param_range(&self, at: usize) -> String {
        param(&self.description, at).map_or_else(String::new, range)
    }

    /// How the page edits parameter `at`.
    #[must_use]
    pub fn param_editor(&self, at: usize) -> Editor {
        match param(&self.description, at).map(|spec| spec.kind) {
            Some(ParamKind::Choice { .. }) => Editor::Choice,
            Some(ParamKind::Toggle) => Editor::Toggle,
            _ => Editor::Number,
        }
    }

    /// A choice's names, in order; empty for another parameter.
    #[must_use]
    pub fn param_choices(&self, at: usize) -> Vec<String> {
        param(&self.description, at).map_or_else(Vec::new, |spec| spec.choices.clone())
    }

    /// Whether parameter `at` acts now; the page greys a face of a setting
    /// the screen does not show. A refresh keeps it current.
    #[must_use]
    pub fn param_active(&self, at: usize) -> bool {
        param(&self.description, at).is_some_and(|spec| spec.active)
    }

    /// Whether parameter `at` takes a CV input.
    #[must_use]
    pub fn param_assignable(&self, at: usize) -> bool {
        param(&self.description, at).is_some_and(|spec| spec.assignable)
    }

    /// The request that sets parameter `at` to what `text` says, as
    /// `cargo probe set` reads it.
    ///
    /// # Errors
    ///
    /// What the text should have looked like.
    pub fn set_request(&self, at: usize, text: &str) -> Result<Vec<u8>, String> {
        let spec = param(&self.description, at).ok_or("no such parameter")?;
        let value = parse_value(spec, text)?;
        Ok(frame(&Request::SetParam {
            app: self.app,
            param: spec.index,
            value,
        }))
    }

    /// Takes the value the module answered for parameter `at` and keeps it;
    /// its text, or an empty one if the message carried none.
    pub fn absorb_param(&mut self, at: usize, message: &Message) -> String {
        let Some(Applet::Value(value)) = message.applet else {
            return String::new();
        };
        let Some(spec) = self.description.params.get_mut(at) else {
            return String::new();
        };
        spec.value = Some(value);
        value_text(spec, value)
    }

    /// How many observables.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn observable_count(&self) -> usize {
        self.description.observables.len()
    }

    /// Observable `at`'s name.
    #[must_use]
    pub fn observable_name(&self, at: usize) -> String {
        observable(&self.description, at).map_or_else(String::new, |spec| spec.name.clone())
    }

    /// Observable `at`'s value as the module writes it; `--` while unknown.
    #[must_use]
    pub fn observable_text(&self, at: usize) -> String {
        observable(&self.description, at).map_or_else(String::new, shown)
    }

    /// The request that reads observable `at` now.
    #[must_use]
    pub fn observe_request(&self, at: usize) -> Vec<u8> {
        let index = observable(&self.description, at).map_or(u8::MAX, |spec| spec.index);
        frame(&Request::Observe {
            app: self.app,
            index,
        })
    }

    /// Takes the value the module answered for observable `at` and keeps
    /// it; its text.
    pub fn absorb_observed(&mut self, at: usize, message: &Message) -> String {
        let Some(Applet::Observed(value)) = message.applet else {
            return String::new();
        };
        let Some(spec) = self.description.observables.get_mut(at) else {
            return String::new();
        };
        spec.value = value;
        shown(spec)
    }

    /// The actions' names, in order.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn actions(&self) -> Vec<String> {
        self.description.actions.clone()
    }

    /// The request that does action `at`.
    #[must_use]
    pub fn act_request(&self, at: usize) -> Vec<u8> {
        frame(&Request::Act {
            app: self.app,
            action: u8::try_from(at).unwrap_or(u8::MAX),
        })
    }

    /// The request that hands this applet the outputs and the screen.
    #[must_use]
    pub fn select_request(&self) -> Vec<u8> {
        frame(&Request::SelectApp { app: self.app })
    }
}

fn param(description: &AppDescription, at: usize) -> Option<&SpecEntry> {
    description.params.get(at)
}

fn observable(description: &AppDescription, at: usize) -> Option<&SpecEntry> {
    description.observables.get(at)
}

/// An entry's value as text, `--` while unknown.
fn shown(spec: &SpecEntry) -> String {
    spec.value
        .map_or_else(|| "--".to_owned(), |value| value_text(spec, value))
}

/// The values an entry takes, as text.
fn range(spec: &SpecEntry) -> String {
    match spec.kind {
        ParamKind::Number { min, max, .. } => format!(
            "{} .. {}",
            value_text(spec, ParamValue::Number(min)),
            value_text(spec, ParamValue::Number(max))
        ),
        ParamKind::Choice { .. } => spec.choices.join(" | "),
        ParamKind::Toggle => "ON | OFF".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use oc_core::apps::AppId;
    use oc_core::apps::ratchet::RatchetApp;
    use oc_core::testing::{MockEngine, mock_engine};
    use oc_protocol::server::{Identity, Server};
    use oc_protocol::{Request, VERSION};

    use super::{AppList, AppView, Editor};
    use crate::{Message, ProbeReader, frame};

    /// A module end: the server over a mock engine, and the page's reader.
    struct Module {
        server: Server,
        engine: MockEngine,
        reader: ProbeReader,
    }

    impl Module {
        fn new() -> Self {
            let mut module = Self {
                server: Server::new(Identity {
                    firmware: "0.6.0",
                    build: "abc1234",
                    can_reboot: false,
                }),
                engine: mock_engine(0),
                reader: ProbeReader::new(),
            };
            module.send(&frame(&Request::Hello { protocol: VERSION }));
            module
        }

        /// What the page receives for `bytes`.
        fn send(&mut self, bytes: &[u8]) -> Vec<Message> {
            self.server.receive(bytes, &mut self.engine);
            let queue = self.server.outgoing();
            let waiting = queue.peek(usize::MAX).to_vec();
            queue.consume(waiting.len());
            self.reader.push(&waiting)
        }
    }

    #[test]
    fn the_list_names_every_applet_and_the_running_one() {
        let mut module = Module::new();
        let mut list = AppList::new();
        let messages = module.send(&frame(&Request::ListApps));
        let done = messages.iter().map(|message| list.absorb(message)).last();
        assert_eq!(done, Some(true));
        assert_eq!(list.len(), AppId::ALL.len());
        assert_eq!(list.name(3), "RATCHET");
        assert!(list.running(0) && !list.running(3));
    }

    #[test]
    fn a_view_shows_edits_and_sets_as_the_command_line_does() {
        let mut module = Module::new();
        let ratchet = u8::try_from(AppId::Ratchet.index()).unwrap();
        let mut view = AppView::new(ratchet);
        for message in module.send(&view.describe_request()) {
            if view.absorb(&message) {
                break;
            }
        }
        assert!(!view.broken());
        assert_eq!(view.param_name(4), "SHAPE");
        assert_eq!(view.param_text(4), "0.0 OCT");
        assert_eq!(view.param_edit_text(4), "0.0");
        assert_eq!(view.param_range(4), "-3.0 OCT .. 3.0 OCT");
        assert_eq!(view.param_editor(8), Editor::Choice);
        assert_eq!(view.param_choices(8), ["FIRST", "SPAN"]);
        assert!(view.param_assignable(0) && !view.param_assignable(1));
        assert_eq!(view.actions(), ["BURST"]);
        assert_eq!(view.param_label(2), "FIRST");
        assert!(
            view.param_active(2) && !view.param_active(3),
            "SYNC OFF: FIRST acts"
        );

        // SYNC ON: DIVISION acts instead, and a refresh says so.
        module.send(&view.set_request(7, "on").unwrap());
        for message in module.send(&view.refresh_request()) {
            if view.absorb_refresh(&message) {
                break;
            }
        }
        assert!(!view.param_active(2) && view.param_active(3));
        module.send(&view.set_request(7, "off").unwrap());

        // MODE SPAN: the time reads SPAN on the screen, and so on the page.
        module.send(&view.set_request(8, "span").unwrap());
        for message in module.send(&view.refresh_request()) {
            if view.absorb_refresh(&message) {
                break;
            }
        }
        assert_eq!(
            (view.param_name(2).as_str(), view.param_label(2).as_str()),
            ("FIRST", "SPAN")
        );
        module.send(&view.set_request(8, "first").unwrap());

        // GATE off puts COUNT at its last: a refresh shows it, as the module does.
        let count = 0;
        let gate = 1;
        view.settle();
        module.send(&view.set_request(gate, "on").unwrap());
        module.send(&view.set_request(gate, "off").unwrap());
        for message in module.send(&view.refresh_request()) {
            if view.absorb_refresh(&message) {
                break;
            }
        }
        assert_eq!(view.param_text(count), "32");
        assert!(view.param_moved(count), "COUNT moved with GATE");
        assert!(!view.param_moved(4), "SHAPE did not");

        let request = view.set_request(4, "-1.5").unwrap();
        let answer = module.send(&request);
        assert_eq!(view.absorb_param(4, &answer[0]), "-1.5 OCT");
        assert_eq!(
            module.engine.app::<RatchetApp>().settings().curve_tenths,
            -15
        );
        assert!(view.set_request(4, "steep").is_err());

        let rate = module.send(&view.observe_request(0));
        assert_eq!(view.absorb_observed(0, &rate[0]), "--");
        module.send(&view.select_request());
        assert_eq!(module.engine.current_app(), AppId::Ratchet);
    }
}
