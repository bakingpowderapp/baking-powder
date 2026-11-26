use super::*;
use super::automation::Armed;
use super::generator::Generator;

remembered!(View {
    flag palette_shown: bool = false, "palette";
    flag big: bool = false, "big";
    flag palette_big: bool = false, "palette_big";
    choice auto: Auto = Auto::Off, "auto";
    whole bank: usize = 0, "bank", 0..=i32::MAX, |bank| bank % SEND_BANKS;
    flag wave_loop: bool = false, "wave_loop";
    choice aside: Aside = Aside::Spectrum, "aside";
    flag browser_big: bool = false, "browser_big";
});

pub(super) fn return_name(host: Host) -> String {
    match host {
        Host::Send(send) => format!("SEND {}", SEND_NAMES[send]),
        Host::Track(_) | Host::Master => String::from("MASTER"),
    }
}

impl App {
    pub(super) fn panel_host(&self) -> Host {
        match self.monitored {
            Some(Host::Send(send)) => Host::Send(send),
            _ => Host::Master,
        }
    }

    fn armed_slot(&mut self, owner: Host) -> &mut Option<Armed> {
        match owner {
            Host::Track(track) => &mut self.generator_mut(track).armed,
            Host::Send(send) => &mut self.armed_returns[send],
            Host::Master => &mut self.armed_returns[SEND_COUNT],
        }
    }

    pub(super) fn carry_armed(&mut self, from: Host, to: Host) {
        if from == to {
            return;
        }
        *self.armed_slot(from) = self.armed.take();
        self.armed = self.armed_slot(to).clone();
        self.lfo_face.close();
        self.lfo_strip = None;
    }

    pub(super) fn show(&mut self, open: Option<usize>) {
        self.open = open;
        self.face.close();
        self.rack.close();
        if let Some(row) = open {
            self.rack.reveal(self.project.rack(self.host()), row);
        }
        self.picture.reset();
    }

    pub(super) fn host(&self) -> Host {
        self.monitored.unwrap_or(Host::Track(self.selection.track))
    }

    pub(super) fn generator(&self, track: usize) -> Generator {
        self.generators.get(track).cloned().unwrap_or_default()
    }

    pub(super) fn generator_mut(&mut self, track: usize) -> &mut Generator {
        if self.generators.len() <= track {
            self.generators.resize_with(track + 1, Generator::default);
        }
        &mut self.generators[track]
    }

    pub(super) fn solos(&self) -> Vec<Option<CoreSolo>> {
        self.generators.iter().map(|generator| generator.core).collect()
    }

    pub(super) fn leave(&mut self) {
        if self.monitored.is_none() {
            let (track, open) = (self.selection.track, self.open);
            self.generator_mut(track).open = open;
        }
    }

    pub(super) fn arrive(&mut self) {
        let generator = self.generator(self.selection.track);
        self.rack.rewind();
        self.show(generator.open);
    }

    pub(super) fn monitored_send(&self) -> Option<usize> {
        match self.monitored {
            Some(Host::Send(send)) => Some(send),
            _ => None,
        }
    }

    pub(super) fn shown(&self) -> Option<(Host, usize)> {
        let host = self.host();
        let rack = self.project.rack(host);
        let row = self.open.filter(|row| *row < rack.len()).or_else(|| match host {
            Host::Track(track) if self.project.tracks[track].generates() => Some(self.project.tracks[track].first_core()),
            Host::Track(_) | Host::Send(_) | Host::Master => (!rack.is_empty()).then_some(0),
        })?;
        Some((host, row))
    }

    pub(super) fn readout_rows(&self) -> i32 {
        self.shown().map_or(face::READOUT_ROWS, |(host, row)| self.project.rack(host)[row].faced().readout_rows())
    }

    pub(super) fn core_shown(&self) -> bool {
        self.screen == Screen::Main && !self.small() && !self.view.big && !self.view.palette_shown && !self.roll_shown()
    }

    pub(super) fn watched_insert(&self) -> Target {
        self.shown()
            .filter(|&(host, row)| self.core_shown() && matches!(self.project.rack(host)[row], Row::Effect(slot) if matches!(slot.kind(), Kind::Eq | Kind::Space | Kind::Freak | Kind::Motion | Kind::Suppressor)))
            .map_or(Target::NONE, |(host, row)| self.audio.as_ref().map_or(Target::NONE, |audio| audio.tapped(host, row)))
    }

    pub(super) fn heard(&self) -> Target {
        if self.view.aside == Aside::Spectrum && self.core_shown() { Host::Master.output() } else { Target::NONE }
    }

    pub(super) fn set_aside(&mut self, aside: Aside) {
        if self.view.aside == Aside::Browser && aside != Aside::Browser {
            self.browser.close();
        }
        self.view.aside = aside;
    }

    pub(super) fn browser_shown(&self) -> bool {
        self.view.aside == Aside::Browser && self.core_shown()
    }

    pub(super) fn browser_big(&self) -> bool {
        self.view.browser_big && self.browser_shown()
    }

    pub(super) fn shown_midi(&self) -> Option<(Host, usize)> {
        self.shown().filter(|&(host, row)| self.project.rack(host)[row].is_midi())
    }

    pub(super) fn lanes_shown(&self) -> bool {
        self.view.aside == Aside::Pattern && self.shown_midi().is_some()
    }

    pub(super) fn big_offered(&self) -> bool {
        self.shown().is_some_and(|(host, row)| matches!(&self.project.rack(host)[row], Row::Effect(slot) if slot.goes_big()))
    }

    pub(super) fn picture_big(&self) -> bool {
        self.view.aside == Aside::Picture && self.big_offered()
    }

    pub(super) fn own_button(&self) -> Option<(&'static str, Aside)> {
        if self.shown_midi().is_some() {
            Some(("P", Aside::Pattern))
        } else if self.big_offered() {
            Some(("BIG", Aside::Picture))
        } else {
            None
        }
    }

    pub(super) fn track_names(&self) -> Vec<String> {
        let named = |(index, track): (usize, &Track)| if track.name.is_empty() { format!("{} {}", track.kind(), index + 1) } else { track.name.clone() };
        std::iter::once(String::from("off")).chain(self.project.tracks.iter().enumerate().map(named)).collect()
    }

    pub(super) fn flat_refusal(&self) -> Option<&'static str> {
        match self.shown()? {
            (Host::Track(track), row) if self.project.tracks[track].rack.get(row).is_some_and(Row::is_midi) => flatten::refusal(&self.project, track, row),
            _ => None,
        }
    }

    pub(super) fn withheld_name(&self) -> Option<usize> {
        match self.shown() {
            Some((Host::Track(track), row)) if matches!(self.project.tracks[track].rack[row], Row::Effect(slot) if slot.kind() == Kind::Gate) => Some(track + 1),
            _ => None,
        }
    }

    pub(super) fn on_audio(&self) -> bool {
        self.project.tracks.get(self.selection.track).is_some_and(|track| track.audio)
    }

    pub(super) fn wave_offered(&self) -> bool {
        self.on_audio() || self.shown_clip().is_some()
    }

    pub(super) fn over_generator(&self, x: i32, y: i32) -> bool {
        self.core_shown() && (self.face_rect().contains(x, y) || self.rack_rect().contains(x, y))
    }
}
