use super::*;
use crate::factory;
use crate::project::Keeping;
use std::sync::OnceLock;

const DEFAULT_OPTION: &str = "Default";
const EFFECTS_OPTION: &str = "Effects";
pub(super) const STOCK_DESC: &str = "Factory presets for this effect; press one to hear it";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Saving {
    Row,
    Rack,
    Effects,
    Default,
}

impl Saving {
    fn label(self, row: &Row) -> &'static str {
        match self {
            Saving::Row => row.label(),
            Saving::Rack => Preset::RACK,
            Saving::Effects => EFFECTS_OPTION,
            Saving::Default => DEFAULT_OPTION,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct SavePanel {
    pub(super) pressed: Option<usize>,
}

#[derive(Clone, Copy)]
pub(super) struct StockPanel {
    pub(super) kind: Kind,
    pub(super) pressed: Option<usize>,
    pub(super) chosen: Option<usize>,
}

pub(super) struct Stocked {
    pub(super) kind: Kind,
    pub(super) name: String,
    pub(super) description: String,
    preset: Preset,
}

impl Stocked {
    fn read(path: &str, text: &str) -> Option<Stocked> {
        let json = json::parse(text)?;
        let description = json.get("description").and_then(Json::as_str).map(String::from).unwrap_or_default();
        let Document::Preset(preset) = Document::from_json(&json)? else {
            return None;
        };
        let Preset::Row(Row::Effect(slot), ..) = &preset else {
            return None;
        };
        Some(Stocked { kind: slot.kind(), name: stem_of(path), description, preset })
    }

    fn holds(&self, slot: &Slot) -> bool {
        matches!(&self.preset, Preset::Row(Row::Effect(own), ..) if Slot { bypass: own.bypass, ..*slot } == *own)
    }
}

pub(super) fn stocked(kind: Kind) -> impl Iterator<Item = &'static Stocked> {
    static STOCK: OnceLock<Vec<Stocked>> = OnceLock::new();
    let stock = STOCK.get_or_init(|| factory::PRESETS.iter().filter(|(path, _)| path.starts_with(factory::EFFECTS)).filter_map(|(path, text)| Stocked::read(path, text)).collect());
    stock.iter().filter(move |entry| entry.kind == kind)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Site {
    Generator,
    Track(usize),
}

impl App {
    pub(super) fn site_at(&self, x: i32, y: i32) -> Option<Site> {
        if self.over_generator(x, y) {
            return Some(Site::Generator);
        }
        self.track_under(x, y).filter(|_| self.screen == Screen::Main).map(Site::Track)
    }

    fn site_host(&self, site: Site) -> Host {
        match site {
            Site::Generator => self.host(),
            Site::Track(track) => Host::Track(track),
        }
    }

    fn swapped(&self, host: Host, preset: &Preset, pressed: bool) -> Option<usize> {
        let rows = preset.rows();
        self.shown().filter(|&(shown_host, row)| pressed && shown_host == host && rows.len() == 1 && self.project.rack(host)[row].same_kind(rows[0])).map(|(_, row)| row)
    }

    pub(super) fn plan(&self, site: Site, preset: &Preset, pressed: bool) -> Result<Plan, Denial> {
        let host = self.site_host(site);
        self.project.plan(host, preset, self.swapped(host, preset, pressed), self.shift)
    }

    fn show_landed(&mut self, landed: Landed) {
        match landed {
            Landed::Sound(_) => self.show(None),
            Landed::Swapped { row, same } => {
                if !same {
                    self.face.close();
                }
                self.pick_rows(vec![row]);
                self.rack_action(Action::Open(row));
            }
            Landed::Cores { first, count } => {
                self.face.close();
                self.lfo_face.close();
                self.lfo_strip = None;
                self.picture.reset();
                if count == 0 {
                    self.show(None);
                } else {
                    self.pick_rows((first..first + count).collect());
                    self.rack_action(Action::Open(first));
                }
            }
            Landed::Joined(landed) => {
                let picked: Vec<usize> = landed.into_iter().flatten().collect();
                if let Some(&last) = picked.last() {
                    self.pick_rows(picked);
                    self.rack_action(Action::Open(last));
                }
            }
        }
    }

    fn landing_deck(&self, at: Option<(i32, i32)>) -> usize {
        at.filter(|_| self.view.palette_shown).and_then(|(x, y)| self.palette_at(x, y)).unwrap_or(0)
    }

    pub(super) fn settle_armed(&mut self) {
        if self.armed_now().is_none() {
            self.armed = None;
        }
    }

    pub(super) fn land_preset(&mut self, name: &str, description: &str, preset: Preset, at: Option<(i32, i32)>) -> Option<Denial> {
        let site = at.and_then(|(x, y)| self.site_at(x, y)).unwrap_or(Site::Generator);
        let plan = match self.plan(site, &preset, at.is_none()) {
            Ok(plan) => plan,
            Err(denial) => return Some(denial),
        };
        let host = self.site_host(site);
        let swapped = self.swapped(host, &preset, at.is_none());
        if let Preset::Chords(columns) = preset {
            self.project.decks[self.landing_deck(at)] = columns;
            self.view.palette_shown = true;
            return None;
        }
        if let Site::Track(track) = site {
            self.select(track);
        }
        self.live.restrike |= preset.rows().into_iter().any(Row::takes_notes);
        let landed = self.project.land(host, preset, plan, swapped, (name, description));
        self.show_landed(landed);
        self.view.palette_shown = false;
        self.selection.clips.clear();
        self.settle_armed();
        None
    }

    pub(super) fn load_preset(&mut self, name: String, description: String, preset: Option<Preset>, at: Option<(i32, i32)>) {
        let Some(preset) = preset else {
            self.refuse(name, UNREADABLE);
            return;
        };
        if let Some(denial) = self.land_preset(&name, &description, preset, at) {
            self.refuse(turned_away(&name, denial), denial.reason());
        }
    }

    fn save_options(&self, host: Host, row: &Row) -> Vec<Saving> {
        let mut options = vec![Saving::Row, Saving::Rack];
        if self.effects_beside_cores(host) {
            options.push(Saving::Effects);
        }
        if row.is_effect() {
            options.push(Saving::Default);
        }
        options
    }

    fn effects_beside_cores(&self, host: Host) -> bool {
        host.track().is_some_and(|track| {
            let rack = &self.project.tracks[track].rack;
            rack.iter().any(Row::is_core) && rack.iter().any(Row::is_effect)
        })
    }

    pub(super) fn save_labels(&self) -> Vec<&'static str> {
        self.shown().map_or_else(Vec::new, |(host, row)| {
            let row = &self.project.rack(host)[row];
            self.save_options(host, row).into_iter().map(|option| option.label(row)).collect()
        })
    }

    pub(super) fn save_preset(&mut self, option: usize) {
        let Some((host, index)) = self.shown() else {
            return;
        };
        let row = self.project.rack(host)[index];
        let (name, keeping) = match self.save_options(host, &row).get(option) {
            Some(Saving::Row) => (row.label(), Keeping::Row(index)),
            Some(Saving::Rack) => (Preset::RACK, Keeping::Rack),
            Some(Saving::Effects) => (EFFECTS_OPTION, Keeping::Effects),
            Some(Saving::Default) => {
                self.keep_default(row);
                return;
            }
            None => return,
        };
        if let Some(preset) = self.project.preset_of(host, keeping) {
            self.request_preset(name, preset);
        }
    }

    fn keep_default(&mut self, row: Row) {
        let Row::Effect(slot) = row else {
            return;
        };
        let kept = Row::Effect(slot);
        match self.defaults.iter_mut().find(|known| known.same_kind(&kept)) {
            Some(known) => *known = kept,
            None => self.defaults.push(kept),
        }
        self.disk.saved_note = Some((files::Saved::Effect(kept.label()), files::SAVED_SECONDS));
    }

    pub(super) fn fresh_effect(&self, kind: Kind) -> Row {
        self.defaults.iter().copied().find(|row| matches!(row, Row::Effect(slot) if slot.kind() == kind)).map_or(Row::Effect(Slot::new(kind)), |row| row.renumbered(|_| None))
    }

    pub(super) fn save_chords(&mut self, deck: usize) {
        self.request_preset(Preset::CHORDS, Preset::Chords(self.project.decks[deck]));
    }

    fn request_preset(&mut self, name: &str, preset: Preset) {
        self.requests.push(Request::Preset { name: format!("{name}.json"), json: preset.to_json().write() });
    }

    fn shown_effect(&self) -> Option<Slot> {
        let (host, row) = self.shown()?;
        match self.project.rack(host)[row] {
            Row::Effect(slot) => Some(slot),
            Row::Midi(_) | Row::Core(_) | Row::Bus(_) => None,
        }
    }

    pub(super) fn stock_kind(&self) -> Option<Kind> {
        self.shown_effect().map(|slot| slot.kind()).filter(|&kind| stocked(kind).next().is_some())
    }

    pub(super) fn stock_offered(&self) -> bool {
        self.stock_kind().is_some()
    }

    pub(super) fn stock_labels(&self) -> Vec<&'static str> {
        self.stock.map_or_else(Vec::new, |panel| stocked(panel.kind).map(|entry| entry.name.as_str()).collect())
    }

    pub(super) fn open_stock(&mut self) {
        let Some((kind, slot)) = self.stock_kind().zip(self.shown_effect()) else {
            return;
        };
        self.stock = Some(StockPanel { kind, pressed: None, chosen: stocked(kind).position(|entry| entry.holds(&slot)) });
    }

    pub(super) fn settle_stock(&mut self) {
        if self.stock.is_some_and(|panel| !self.core_shown() || self.stock_kind() != Some(panel.kind)) {
            self.stock = None;
        }
    }

    pub(super) fn stock_pick(&mut self, index: usize) {
        let Some(panel) = self.stock.filter(|panel| self.stock_kind() == Some(panel.kind)) else {
            self.stock = None;
            return;
        };
        let Some(entry) = stocked(panel.kind).nth(index) else {
            return;
        };
        self.load_preset(entry.name.clone(), entry.description.clone(), Some(entry.preset.clone()), None);
        self.stock = Some(StockPanel { pressed: None, chosen: Some(index), ..panel });
    }

    pub(super) fn stock_key(&mut self, key: Key, repeat: bool) -> Option<bool> {
        let panel = self.stock?;
        match key {
            Key::Escape | Key::Return => {
                self.stock = None;
                Some(true)
            }
            Key::Up | Key::Down => {
                let count = stocked(panel.kind).count();
                let next = match (key, panel.chosen) {
                    _ if repeat => None,
                    (Key::Up, Some(chosen)) => chosen.checked_sub(1),
                    (Key::Up, None) => count.checked_sub(1),
                    (_, Some(chosen)) => Some(chosen + 1).filter(|next| *next < count),
                    (_, None) => (count > 0).then_some(0),
                };
                let Some(next) = next else {
                    return Some(false);
                };
                self.stock_pick(next);
                Some(true)
            }
            _ => None,
        }
    }
}
