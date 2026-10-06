use super::{Context, Happened, State, CHAT_LIMIT};

/// The opening turn: who this is, what it may propose, and this moment. Who
/// this is comes in two authored layers, the package's Personality Prompt then
/// this Instance's own (ADR-0012). Later wakes send `follow_up` only.
///
/// `blank` empties the built-in layers and keeps the user's Instance Prompt:
/// Blank AI is the control run for the shipped prompt. The empty strings keep
/// their seats so the Prompt tab shows what was sent, and the reply is prose.
pub(crate) fn character_prompt(
    context: &Context,
    behaviors: impl IntoIterator<Item = impl AsRef<str>>,
    blank: bool,
) -> String {
    let moment = follow_up(context);

    // Empty, not `(no personality)`: that placeholder is a Character with an
    // empty file, which still gets the voice rules. Blank AI is the emptied
    // built-in layer sitting in the same seat.
    let personality = if blank {
        ""
    } else if context.personality.is_empty() {
        "(no personality)"
    } else {
        context.personality.as_str()
    };

    // Package layer first, Instance second, both ahead of the roster and voice
    // rules so those govern the user's words as they govern the author's. A
    // layer nobody wrote is left out rather than emitted blank (ADR-0012).
    let authored = match context.instance_prompt.trim() {
        "" => personality.to_string(),
        written if personality.is_empty() => written.to_string(),
        written => format!("{personality}\n\n{written}"),
    };

    let instructions = app_instructions(behaviors, blank);

    match (authored.is_empty(), instructions.is_empty()) {
        (true, true) => moment,
        (false, true) => format!("{authored}\n\n{moment}"),
        (true, false) => format!("{instructions}\n\n{moment}"),
        (false, false) => format!("{authored}\n\n{instructions}\n\n{moment}"),
    }
}

/// The app-level layer of the Character Prompt: voice rules, Behavior roster,
/// reply contract. Empty under Blank AI. The Prompt tab draws this frozen so
/// an emptied control run is visible, not a missing block.
///
/// Every word here is paid on every wake, by an HTTP Director with a small
/// context as much as by a Harness, and it crowds the Character and Instance
/// prompts that carry the personality. So it says only what nothing else can:
/// the roster, which no tool schema advertises, and the reply shape the parser
/// needs. The tools describe themselves over MCP, so naming them here would
/// only be a second copy to drift.
pub fn app_instructions(
    behaviors: impl IntoIterator<Item = impl AsRef<str>>,
    blank: bool,
) -> String {
    if blank {
        return String::new();
    }
    let names: Vec<String> = behaviors
        .into_iter()
        .map(|name| name.as_ref().to_string())
        .collect();
    let declared = if names.is_empty() {
        "(none)".to_string()
    } else {
        names.join(", ")
    };
    // The universal voice rules, written once for every Character rather
    // than copied into personality files to drift. A personality
    // supplies the material; this paragraph governs the delivery.
    format!(
        "You may propose one of these behaviors: {declared}\n\
         \n\
         Reply with the behavior name on the first line.\n\
         An optional spoken line may follow on the next line.\n\
         Propose nothing else.\n\
         \n\
         Speak in this character's voice, always in character: never mention \
         being a model or an assistant. A spoken line fits a small speech \
         bubble: five short sentences at the most. Vary: prefer a line you \
         have not used yet, though a signature phrase may recur, and \
         lean away from the behaviors listed as recently played. React to \
         this moment when there is something worth remarking on: what just \
         happened to you, and what you are standing on.\n\
         \n\
         When you are asked for something, use the tools you have. Then \
         reply as above."
    )
}

/// The one word the prompt uses for each `Happened`.
pub fn happened_word(happened: &Happened) -> &'static str {
    match happened {
        Happened::Poke => "poked",
        Happened::Throw => "thrown",
        Happened::Summon => "summoned",
        Happened::Grab => "picked up",
        Happened::Perch => "placed on a perch",
        Happened::Chat(_) => "spoken to",
        Happened::Proactive => "time passed",
    }
}

/// A later turn in the same session. No Personality Prompt, no roster.
pub(crate) fn follow_up(context: &Context) -> String {
    let recent = if context.recent.is_empty() {
        "(none)".to_string()
    } else {
        context.recent.join(", ")
    };
    let clock = format_clock(context.activity.hour, context.activity.minute);
    let happened = happened_word(&context.happened);
    let state = match context.state {
        State::Grounded => "idle",
        State::Falling => "falling",
        State::Dragged => "held",
        State::Perched => "perched",
        State::Climbing => "climbing",
        State::Asleep => "asleep",
    };
    let weekday = WEEKDAYS[usize::from(context.activity.weekday % 7)];
    let desktop = desktop_lines(context);

    // Last, after every labelled fact, because it is the only line the user
    // writes: a paste imitating `state:` or `front:` then reads as part of what
    // was said and cannot displace the real value above it.
    let said = match &context.happened {
        Happened::Chat(line) => format!("they said: {}\n", cut(line, CHAT_LIMIT)),
        _ => String::new(),
    };

    format!(
        "what just happened: {happened}\n\
         recent: {recent}\n\
         time: {weekday} {clock}\n\
         state: {state}\n\
         standing on: {standing}\n\
         {desktop}\
         {said}",
        standing = if context.standing.is_empty() {
            "nothing"
        } else {
            context.standing.as_str()
        },
    )
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Characters of a window title the prompt keeps. A hint, not a record: the
/// Harness can call `list_windows` for the rest.
const TITLE_LIMIT: usize = 24;

/// The `front:`, `before:` and `idle:` lines. Names arrive already gated by
/// consent and the exclusion list, so this decides only how they read.
fn desktop_lines(context: &Context) -> String {
    let activity = &context.activity;
    let mut lines = String::new();
    if let Some(name) = activity.frontmost_application.as_deref().map(flatten) {
        if !name.is_empty() {
            let title = match context.front_title.as_deref().map(flatten) {
                Some(title) if !title.is_empty() => {
                    let short = cut(&title, TITLE_LIMIT);
                    let more = if short.len() < title.len() { "…" } else { "" };
                    format!(" \"{short}{more}\"")
                }
                _ => String::new(),
            };
            let stay = minutes(activity.frontmost_for);
            lines.push_str(&format!("front: {name}{title} {stay}\n"));
        }
    }
    let before: Vec<String> = activity
        .before
        .iter()
        .map(|(name, stayed)| format!("{} {}", flatten(name), minutes(*stayed)))
        .collect();
    if !before.is_empty() {
        lines.push_str(&format!("before: {}\n", before.join(", ")));
    }
    // Under a minute reads as present, and Wayland reports zero always.
    if activity.idle.as_secs() >= 60 {
        lines.push_str(&format!("idle: {}\n", minutes(activity.idle)));
    }
    lines
}

/// Whole minutes, the only resolution the prompt spends tokens on.
fn minutes(lasted: std::time::Duration) -> String {
    format!("{}m", lasted.as_secs() / 60)
}

/// Another application's text, flattened so it cannot start a labelled line
/// of its own or close the quotes around a title. U+2028 is whitespace, not
/// control, and still reads as a line break.
fn flatten(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '"')
        .map(|c| {
            if c.is_control() || c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// `line` at `limit` characters, cut on a character boundary so a multi-byte
/// paste cannot panic the slice.
fn cut(line: &str, limit: usize) -> &str {
    match line.char_indices().nth(limit) {
        Some((end, _)) => &line[..end],
        None => line,
    }
}

fn format_clock(hour: u8, minute: u8) -> String {
    format!("{hour:02}:{minute:02}")
}
