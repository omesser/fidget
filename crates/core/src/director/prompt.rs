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
        Happened::Menu => "menu opened",
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
    let open = match context.activity.frontmost_application.as_deref() {
        Some(name) if !name.is_empty() => format!("{name} is the frontmost window"),
        _ => "nothing is frontmost".to_string(),
    };

    // Last, after every labelled fact, because it is the only line the user
    // writes: a paste imitating `state:` or `open:` then reads as part of what
    // was said and cannot displace the real value above it.
    let said = match &context.happened {
        Happened::Chat(line) => format!("they said: {}\n", cut(line)),
        _ => String::new(),
    };

    format!(
        "what just happened: {happened}\n\
         recent: {recent}\n\
         time: {clock}\n\
         state: {state}\n\
         standing on: {standing}\n\
         open: {open}\n\
         {said}",
        standing = if context.standing.is_empty() {
            "nothing"
        } else {
            context.standing.as_str()
        },
    )
}

/// `line` at `CHAT_LIMIT` characters, cut on a character boundary so a
/// multi-byte paste cannot panic the slice.
fn cut(line: &str) -> &str {
    match line.char_indices().nth(CHAT_LIMIT) {
        Some((end, _)) => &line[..end],
        None => line,
    }
}

fn format_clock(hour: u8, minute: u8) -> String {
    format!("{hour:02}:{minute:02}")
}
