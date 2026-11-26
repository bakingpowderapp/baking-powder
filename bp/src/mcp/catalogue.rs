use super::{EXPORT, LIBRARY, LISTEN, object, text};
use sdk::json::Json;

pub const INSTRUCTIONS: &str = include_str!("instructions.md");
pub const GUIDE: &str = include_str!("guide.md");
pub(super) const ASK: &str = "If this MCP could be improved, say how in your reply.";
const MOST_INSTRUCTION_BYTES: usize = 4096;
const _: () = assert!(INSTRUCTIONS.len() + 2 + ASK.len() <= MOST_INSTRUCTION_BYTES, "Claude Code keeps only the first 4,096 characters of a server's instructions, so the rest belongs in the guide");

pub(super) fn instructions(asking: bool) -> String {
    if asking { format!("{INSTRUCTIONS}\n\n{ASK}") } else { String::from(INSTRUCTIONS) }
}

#[derive(Clone, Copy)]
enum Shape {
    Text,
    Number,
    Flag,
    List,
    Pairs,
    Pair,
    Any,
}

struct Arg {
    name: &'static str,
    shape: Shape,
    about: &'static str,
    required: bool,
}

#[derive(Clone, Copy)]
enum Effect {
    Reads,
    Keeps,
    Edits,
    Moves,
}

struct Tool {
    name: &'static str,
    about: &'static str,
    args: &'static [Arg],
    effect: Effect,
}

const PATH: Arg = Arg { name: "path", shape: Shape::Text, about: "Zero-based document path, e.g. tracks/2/clips/0. Omit for the whole project.", required: false };
const NOTES: Arg = Arg { name: "notes", shape: Shape::Flag, about: "Expand notes; otherwise return folded references.", required: false };
const OUTLINE: Arg = Arg { name: "outline", shape: Shape::Flag, about: "Also fold clips, racks, curves, LFOs and palette; notes true keeps clips and notes.", required: false };
const SINCE: Arg = Arg { name: "since", shape: Shape::Number, about: "Return changes since this revision, optionally under path; the reply's now is the current revision.", required: false };
const READ_REVISION: Arg = Arg { name: "revision", shape: Shape::Number, about: "Read path as it stood at this revision, among the last 32 kept; write the value back to restore it.", required: false };
const READ_FROM: Arg = Arg { name: "fromBeat", shape: Shape::Number, about: "With untilBeat, sample the knob, gain or send at path across these beats: curves and LFOs combined.", required: false };
const READ_UNTIL: Arg = Arg { name: "untilBeat", shape: Shape::Number, about: "End beat for sampling, after fromBeat.", required: false };
const WRITE_PATH: Arg = Arg { name: "path", shape: Shape::Text, about: "Destination; end appends; omit for the whole project. Beat operations take tracks/N, or every track, return and master.", required: false };
const VALUE: Arg = Arg { name: "value", shape: Shape::Any, about: "Replacement JSON; null removes. Whole projects require kind: project.", required: false };
const EDITS: Arg = Arg { name: "edits", shape: Shape::List, about: "Ordered writes as one undo step. Each has one operation: value, scale, transpose, fit, insert, remove, copy, cut, join, duplicate, move, match, flatten, quantize, double, halve, reverse or chords, with its arguments.", required: false };
const WRITE_DETAILS: Arg = Arg { name: "details", shape: Shape::Flag, about: "Include paths, project totals and individual import/preset/pattern confirmations.", required: false };
const SCALE: Arg = Arg { name: "scale", shape: Shape::Number, about: "Multiply curve values, or clip/track velocities and wave levels (factor above 0, clamped to 1–127 and ±24 dB).", required: false };
const TRANSPOSE: Arg = Arg { name: "transpose", shape: Shape::Number, about: "Integer semitones for a clip, a track or, with no path, the whole song's notes, root moving too and waves left alone; refused past pitch 0–127 or wave ±24.", required: false };
const FIT: Arg = Arg { name: "fit", shape: Shape::Pair, about: "[lowest, highest] pitches spanning twelve or more, for a clip, a track or, with no path, the whole song: notes outside move by octaves to the nearest pitch inside, pitch classes kept, doubles dropped.", required: false };
const INSERT: Arg = Arg { name: "insert", shape: Shape::Any, about: "Beats of silence at fromBeat, splitting clips, shifting later content; or a track object at tracks/N, renumbering references; or a row object at a rack row, the rows below moving down with their lanes.", required: false };
const REMOVE: Arg = Arg { name: "remove", shape: Shape::Number, about: "Beats to delete at fromBeat, splitting clips and shifting later content earlier.", required: false };
const COPY: Arg = Arg { name: "copy", shape: Shape::Number, about: "Beats from fromBeat to paste at toBeat, overwriting; curves, those on LFO settings too, repeat with each copy unless automationLock.", required: false };
const CUT: Arg = Arg { name: "cut", shape: Shape::Any, about: "A song beat, or a list of them: the clip under each splits in two at tracks/N, or on every track, as holding a clip does in the app.", required: false };
const JOIN: Arg = Arg { name: "join", shape: Shape::Any, about: "Beats from fromBeat to merge into one clip per track, notes shifted; audio joins only when its clips play on from one wave. Or, with no path, a list of MIDI clip paths, one a track, stacked into one clip on the topmost.", required: false };
const DUPLICATE: Arg = Arg { name: "duplicate", shape: Shape::Flag, about: "At tracks/N, insert a copy after the track, a group with its members, renumbering references; at tracks/N/clips/K, lay the clip again right after itself.", required: false };
const MOVE: Arg = Arg { name: "move", shape: Shape::Number, about: "At tracks/N or a rack row, the index it moves to: a group takes its members, track references renumber, and a row's lanes follow it.", required: false };
const MATCH: Arg = Arg { name: "match", shape: Shape::Flag, about: "At an audio clip, give its fadeIn, fadeOut, level spans, semitones and cents to every clip on its track that sounds like it; at a sidechain row, give its settings to every sidechain row taking the signal from the same track.", required: false };
const FLATTEN: Arg = Arg { name: "flatten", shape: Shape::Flag, about: "At a midi row with its arp on, print the notes the row sends, as they play, into the track's clips and switch the arp off, as FLATTEN on the row's face; never refused.", required: false };
const QUANTIZE: Arg = Arg { name: "quantize", shape: Shape::Number, about: "A grid in beats, as 0.25 for sixteenths: snap the start and end of each note of a MIDI clip, a track or, with no path, the whole song to it, as quantize in the piano roll.", required: false };
const DOUBLE: Arg = Arg { name: "double", shape: Shape::Flag, about: "At a MIDI clip, double its length, its notes stretching with it; refused when the next clip leaves no room.", required: false };
const HALVE: Arg = Arg { name: "halve", shape: Shape::Flag, about: "At a MIDI clip of two beats or more, halve its length, its notes shrinking with it.", required: false };
const REVERSE: Arg = Arg { name: "reverse", shape: Shape::Flag, about: "At a MIDI clip, turn its notes back to front in time, the last heard first.", required: false };
const CHORDS: Arg = Arg { name: "chords", shape: Shape::Flag, about: "At a MIDI clip, replace its notes with chords, the notes each bar strikes gathered into one chord held until the next.", required: false };
const WRITE_FROM: Arg = Arg { name: "fromBeat", shape: Shape::Number, about: "Source beat for insert, remove, copy or join; with transpose, scale, fit or quantize, only notes or nodes from this song beat.", required: false };
const WRITE_UNTIL: Arg = Arg { name: "untilBeat", shape: Shape::Number, about: "With transpose, scale, fit or quantize, only notes or nodes before this song beat.", required: false };
const ABOVE: Arg = Arg { name: "above", shape: Shape::Number, about: "With transpose, scale, fit or quantize, only notes above this pitch; keeps a bass below it.", required: false };
const BELOW: Arg = Arg { name: "below", shape: Shape::Number, about: "With transpose, scale, fit or quantize, only notes below this pitch.", required: false };
const PICKED: Arg = Arg { name: "picked", shape: Shape::Flag, about: "With transpose, scale, fit or quantize at the open clip, only the notes picked in the app.", required: false };
const WRITE_TO: Arg = Arg { name: "toBeat", shape: Shape::Number, about: "Destination beat for copy.", required: false };
const COPIES: Arg = Arg { name: "copies", shape: Shape::Number, about: "Copies laid after the source passage: 1–1024, default 1.", required: false };
const UNCHANGED: Arg = Arg { name: "unchangedSince", shape: Shape::Number, about: "Refuse if the written paths, or what a preset loaded or a rack, row or kind written there replaces, changed since this revision; changes elsewhere are counted in the reply.", required: false };
const PREVIEW: Arg = Arg { name: "preview", shape: Shape::Flag, about: "Report what the write would do, up to 40 changed paths with their values before and after, and the revision to guard it, without writing.", required: false };
const REVISION: Arg = Arg { name: "revision", shape: Shape::Number, about: "Undo only this write; omit to undo the last edit by anyone.", required: false };
const ROW: Arg = Arg { name: "row", shape: Shape::Text, about: "Rack kind, e.g. midi, core, compressor or eq; path then addresses keys within that kind.", required: false };
const MANUAL_PATH: Arg = Arg { name: "path", shape: Shape::Text, about: "Document path, path within row, or guide for conventions. Omit for all keys.", required: false };
const SIFT: Arg = Arg { name: "find", shape: Shape::Text, about: "Match every word against keys or lines, case-insensitively; a rack row's keys are searched with row or a row's path.", required: false };
const BEAT: Arg = Arg { name: "beat", shape: Shape::Number, about: "Start beat; defaults to the start mark.", required: false };
const UNTIL: Arg = Arg { name: "untilBeat", shape: Shape::Number, about: "Set loop from beat to this beat. Omit to retain the loop.", required: false };
const FOLDER: Arg = Arg { name: "path", shape: Shape::Text, about: "Browser folder or file, absolute or folder-prefixed, e.g. Instruments/Sequences. Omit to list roots or search all.", required: false };
const DEEP: Arg = Arg { name: "deep", shape: Shape::Flag, about: "Include files in subfolders.", required: false };
const FIND: Arg = Arg { name: "find", shape: Shape::Text, about: "Recursively match every word in names/descriptions, case-insensitively.", required: false };
const KIND: Arg = Arg { name: "kind", shape: Shape::Text, about: "Filter recursively: wave, midi, preset, project or folder.", required: false };
const HEARTED: Arg = Arg { name: "hearted", shape: Shape::Flag, about: "List only the hearted files, everywhere or under path; find and kind narrow them.", required: false };
const HEART: Arg = Arg { name: "hearted", shape: Shape::Flag, about: "False takes the heart away; default true.", required: false };
const ADD: Arg = Arg { name: "add", shape: Shape::Flag, about: "Ask the user to allow the folder at path in the app, or to pick one; it joins the browser folders.", required: false };
const FILE: Arg = Arg { name: "path", shape: Shape::Text, about: "File or subfolder path from library.", required: true };
const LOVED: Arg = Arg { name: "path", shape: Shape::Text, about: "File path from library; folders take no heart.", required: true };
const WAVE: Arg = Arg { name: "path", shape: Shape::Text, about: ".wav path from library.", required: true };
const NAME: Arg = Arg { name: "name", shape: Shape::Text, about: "New filename including extension, without a folder.", required: true };
const TO: Arg = Arg { name: "to", shape: Shape::Text, about: "Destination within a browser folder; created if missing.", required: true };
const SAVE_TO: Arg = Arg { name: "path", shape: Shape::Text, about: "New .json in a browser folder; omit to save the current file wherever it lives.", required: false };
const PRESET: Arg = Arg { name: "preset", shape: Shape::Text, about: "A path in the song to save as a preset at path, leaving the song as it is: a rack row like tracks/0/rack/2 for that row with its LFO lanes, or tracks/0, returns/0 or master for the whole rack with them.", required: false };
const EFFECTS: Arg = Arg { name: "effects", shape: Shape::Flag, about: "With preset at a track, save its effects alone, the midi and Core rows left out.", required: false };
const DESCRIPTION: Arg = Arg { name: "description", shape: Shape::Text, about: "Needed with preset: twelve words at most saying what the sound is, never naming gear, a brand or how it was made.", required: false };
const OVERWRITE: Arg = Arg { name: "overwrite", shape: Shape::Flag, about: "With preset, replace a preset already at path; any other file there still refuses.", required: false };
const COPY_SAVE: Arg = Arg { name: "copy", shape: Shape::Flag, about: "Write a copy at path; the open project keeps its own file.", required: false };
const EXPORT_TO: Arg = Arg { name: "path", shape: Shape::Text, about: "New .wav or .mid in a browser folder.", required: true };
const RENDER_FROM: Arg = Arg { name: "fromBeat", shape: Shape::Number, about: "Start beat; needs untilBeat. WAV: omit both for the loop, else the whole song. MIDI, or a WAV of one clip: crops to them; omit both for the whole.", required: false };
const RENDER_UNTIL: Arg = Arg { name: "untilBeat", shape: Shape::Number, about: "End beat, after fromBeat; endBeat is the song end.", required: false };
const CLIP: Arg = Arg { name: "clip", shape: Shape::Any, about: "For MIDI: a clip path like tracks/0/clips/2, a track path like tracks/0, a clip object with notes, or a list of these, one MIDI track each. Omit for every track. For WAV: one audio clip's path, written alone as heard, without its track's rack.", required: false };
const BPM: Arg = Arg { name: "bpm", shape: Shape::Number, about: "Tempo written into the .mid; defaults to the song's.", required: false };
const LISTEN_FROM: Arg = Arg { name: "fromBeat", shape: Shape::Number, about: "Start beat; requires untilBeat. Omit both for the whole song.", required: false };
const LISTEN_UNTIL: Arg = Arg { name: "untilBeat", shape: Shape::Number, about: "End beat, greater than fromBeat.", required: false };
const ALONE: Arg = Arg { name: "track", shape: Shape::Number, about: "Solo track index, still sidechained. Omit for the mix and per-track measurements.", required: false };
const CORE: Arg = Arg { name: "row", shape: Shape::Number, about: "With track, hear one Core row of that track alone, as soloing a Core in the rack: its number in the track's rack, the track's effects passed over.", required: false };
const CORE_EFFECTS: Arg = Arg { name: "effects", shape: Shape::Flag, about: "With row, keep the track's effects after the Core heard alone.", required: false };
const RETURN: Arg = Arg { name: "return", shape: Shape::Number, about: "Solo one return, 0–15, fed by every send; nothing else sounds. Omit for the mix.", required: false };
const SPANS: Arg = Arg { name: "spans", shape: Shape::Pairs, about: "Up to 64 [fromBeat, untilBeat] pairs, e.g. [[0,8],[8,16]], inside the render, reported in this order; edges fall on 10 ms hops, so a slice or a gap measures alone.", required: false };
const AGAINST: Arg = Arg { name: "against", shape: Shape::Number, about: "Also render this revision, among the last 32 kept, and subtract it sample by sample; null reports the difference, peakDb -120 a full null.", required: false };
const LISTEN_DETAILS: Arg = Arg { name: "details", shape: Shape::Flag, about: "Include bars even with spans, and dynamics with no reduction.", required: false };
const LISTEN_TO: Arg = Arg { name: "path", shape: Shape::Text, about: "Also keep the render as a new .wav in a browser folder, for the user to hear.", required: false };
const DIFFERENCE: Arg = Arg { name: "difference", shape: Shape::Text, about: "With against, a new .wav in a browser folder that keeps this render less that revision's: everything that changed, for the user to hear.", required: false };

const TOOLS: &[Tool] = &[
    Tool { name: "read", about: "Read project state, selection and transport; whole reads include revision, path reads give it after the value. Notes fold unless requested or path enters clips; default large reads auto-outline. Absent keys report defaults. fromBeat and untilBeat sample a modulated key.", args: &[PATH, NOTES, OUTLINE, SINCE, READ_REVISION, READ_FROM, READ_UNTIL], effect: Effect::Reads },
    Tool { name: "write", about: "Replace JSON or apply an operation as one undo step. For presets, wave paths and note patterns, see manual path guide.", args: &[VALUE, WRITE_PATH, EDITS, SCALE, TRANSPOSE, FIT, INSERT, REMOVE, COPY, CUT, JOIN, DUPLICATE, MOVE, MATCH, FLATTEN, QUANTIZE, DOUBLE, HALVE, REVERSE, CHORDS, WRITE_FROM, WRITE_UNTIL, WRITE_TO, COPIES, ABOVE, BELOW, PICKED, UNCHANGED, PREVIEW, WRITE_DETAILS], effect: Effect::Edits },
    Tool { name: "manual", about: "Key types, ranges and defaults; narrow with path, row or find. Large sections fold.", args: &[MANUAL_PATH, ROW, SIFT], effect: Effect::Reads },
    Tool { name: "play", about: "Start playback from beat or the start mark, restarting if playing; untilBeat sets a repeating loop.", args: &[BEAT, UNTIL], effect: Effect::Edits },
    Tool { name: "stop", about: "Stop playback.", args: &[], effect: Effect::Edits },
    Tool { name: "undo", about: "Undo an edit; a revision undo keeps later edits and refuses conflicts.", args: &[REVISION], effect: Effect::Edits },
    Tool { name: "redo", about: "Redo the last undone edit.", args: &[], effect: Effect::Edits },
    Tool { name: LIBRARY, about: "Browse, search or read files. MIDI parts are writable clips. Entries are relative to their folder. Listings cap at 200 entries and 100,000 files walked; narrow path or find.", args: &[FOLDER, DEEP, FIND, KIND, HEARTED, NOTES, ADD], effect: Effect::Reads },
    Tool { name: "import", about: "Import a WAV; returns metadata and a ready clip, warped to the song when the file's name or length gives its tempo. write also imports wave.path.", args: &[WAVE], effect: Effect::Edits },
    Tool { name: "heart", about: "Heart a file under a browser folder, as the browser's heart does. Hearts outlive the session; library marks them.", args: &[LOVED, HEART], effect: Effect::Edits },
    Tool { name: "rename", about: "Rename a file or subfolder within a browser folder.", args: &[FILE, NAME], effect: Effect::Moves },
    Tool { name: "move", about: "Move a file or subfolder within browser folders.", args: &[FILE, TO], effect: Effect::Moves },
    Tool { name: "trash", about: "Trash a file or subfolder within a browser folder.", args: &[FILE], effect: Effect::Moves },
    Tool { name: "save", about: "Save project, waves and versions; copy true leaves the project on its own file. New paths make missing folders, never overwrite files or touch Factory. preset saves a rack row or a rack as a preset instead, with a description.", args: &[SAVE_TO, COPY_SAVE, PRESET, EFFECTS, DESCRIPTION, OVERWRITE], effect: Effect::Edits },
    Tool { name: EXPORT, about: "Export WAV, of the song or one audio clip, or MIDI from clips, tracks or the whole song, one MIDI track each; returns once written. A WAV of the song stops playback. Makes missing folders; cannot overwrite files or target Factory.", args: &[EXPORT_TO, CLIP, BPM, RENDER_FROM, RENDER_UNTIL], effect: Effect::Edits },
    Tool { name: LISTEN, about: "Render and measure; the reply holds no audio: loudness, peaks, stereo, bands, dynamics and flags, per track, return and bar or span; spans add peakBeat and hits, each where its attack starts, its peak and its brightness; against nulls the render with a revision's; path or difference keeps a .wav. Track and return meters precede master. Playback keeps running. See manual path guide.", args: &[LISTEN_FROM, LISTEN_UNTIL, ALONE, CORE, CORE_EFFECTS, RETURN, SPANS, AGAINST, LISTEN_DETAILS, LISTEN_TO, DIFFERENCE], effect: Effect::Keeps },
];

pub(super) fn known(name: &str) -> bool {
    TOOLS.iter().any(|tool| tool.name == name)
}

pub(super) fn edits(name: &str) -> bool {
    TOOLS.iter().any(|tool| tool.name == name && matches!(tool.effect, Effect::Edits | Effect::Moves))
}

pub(super) fn listed() -> Json {
    Json::Array(TOOLS.iter().map(|tool| object(vec![("name", text(tool.name)), ("description", text(tool.about)), ("inputSchema", schema(tool)), ("annotations", annotations(tool.effect))])).collect())
}

fn annotations(effect: Effect) -> Json {
    object(vec![("readOnlyHint", Json::Bool(matches!(effect, Effect::Reads))), ("destructiveHint", Json::Bool(matches!(effect, Effect::Moves))), ("openWorldHint", Json::Bool(false))])
}

fn schema(tool: &Tool) -> Json {
    let properties = tool.args.iter().map(|arg| (String::from(arg.name), shape(arg))).collect();
    let required = tool.args.iter().filter(|arg| arg.required).map(|arg| text(arg.name)).collect();
    object(vec![("type", text("object")), ("properties", Json::Object(properties)), ("required", Json::Array(required))])
}

fn shape(arg: &Arg) -> Json {
    let mut fields = match arg.shape {
        Shape::Text => vec![("type", text("string"))],
        Shape::Number => vec![("type", text("number"))],
        Shape::Flag => vec![("type", text("boolean"))],
        Shape::List => vec![("type", text("array")), ("items", object(vec![("type", text("object"))]))],
        Shape::Pairs => vec![("type", text("array")), ("items", object(vec![("type", text("array")), ("items", object(vec![("type", text("number"))])), ("minItems", Json::Number(2.0)), ("maxItems", Json::Number(2.0))]))],
        Shape::Pair => vec![("type", text("array")), ("items", object(vec![("type", text("number"))])), ("minItems", Json::Number(2.0)), ("maxItems", Json::Number(2.0))],
        Shape::Any => Vec::new(),
    };
    fields.push(("description", text(arg.about)));
    object(fields)
}
