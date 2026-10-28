use super::{Context, Doing, Happened, Lately, State, Step, CHAT_LIMIT};

/// The opening turn: Personality Prompt, Instance Prompt (ADR-0012), roster,
/// the Instance's own id and moment; later wakes send `follow_up`. `blank`
/// empties the built-in layers for the control run, keeping the rest; its
/// reply is prose.
pub(crate) fn character_prompt(
    context: &Context,
    instance: &str,
    behaviors: impl IntoIterator<Item = impl AsRef<str>>,
    blank: bool,
) -> String {
    // Every tool takes this id, and blank mode keeps it: it routes calls to
    // this Instance, which is plumbing rather than voice (#1700).
    let moment = format!(
        "Your instance_id is {instance}. Pass it to every tool you call.\n\n{}",
        follow_up(context)
    );

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

/// The app layer of the Character Prompt: voice rules, roster, reply contract.
/// Empty under Blank AI, which the Prompt tab shows frozen. Paid every wake,
/// so the tools describe themselves over MCP; a copy here would drift.
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
         Reply with behavior: <name> on the first line.\n\
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

/// The line the user typed, read back out of a stored wake prompt: the inverse
/// of `follow_up`'s `they said:`. Searched only after the moment's own header,
/// since the Personality Prompt may quote `they said:` too.
pub fn typed_line(prompt: &str) -> Option<&str> {
    let header = format!(
        "what just happened: {}",
        happened_word(&Happened::Chat(String::new()))
    );
    let at = prompt
        .match_indices(&header)
        .map(|(at, _)| at)
        .find(|&at| at == 0 || prompt[..at].ends_with('\n'))?;
    let moment = &prompt[at..];
    let label = "\nthey said: ";
    let line = &moment[moment.find(label)? + label.len()..];
    Some(line.strip_suffix('\n').unwrap_or(line))
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
    let state = state_word(context.state);
    let caffeinate = match context.caffeinate {
        Some(fact) => format!("{}\n", fact.prompt_line()),
        None => String::new(),
    };
    let weekday = WEEKDAYS[usize::from(context.activity.weekday % 7)];
    let desktop = desktop_lines(context);
    let (streak, was, lately) = lately_lines(context);

    // Last, after every labelled fact, because it is the only line the user
    // writes: a paste imitating `state:` or `front:` then reads as part of what
    // was said and cannot displace the real value above it.
    let said = match &context.happened {
        Happened::Chat(line) => format!("they said: {}\n", cut(line, CHAT_LIMIT)),
        _ => String::new(),
    };

    format!(
        "what just happened: {happened}{streak}\n\
         {was}\
         recent: {recent}\n\
         time: {weekday} {clock}\n\
         state: {state}\n\
         {caffeinate}\
         {lately}\
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

fn state_word(state: State) -> &'static str {
    match state {
        State::Grounded => "idle",
        State::Falling => "falling",
        State::Dragged => "held",
        State::Perched => "perched",
        State::Climbing => "climbing",
        State::Asleep => "asleep",
    }
}

/// The streak suffix, the `was:` line, and the `doing:` and `lately:` lines.
/// `was:` equal to now is left out, and so is a window of one step: the line
/// beside it already says that much.
fn lately_lines(context: &Context) -> (String, String, String) {
    let Lately {
        was,
        doing,
        streak,
        steps,
    } = &context.lately;
    let streak = match streak {
        0 | 1 => String::new(),
        times => format!(" ({times} in a row)"),
    };
    let now = Doing {
        state: context.state,
        behavior: doing.clone(),
    };
    let was = match was {
        Some(found) if *found != now => format!("was: {}\n", doing_words(found)),
        _ => String::new(),
    };
    let mut lines = String::new();
    if let Some(name) = doing {
        lines.push_str(&format!("doing: {name}\n"));
    }
    if steps.len() > 1 {
        let words: Vec<&str> = steps
            .iter()
            .map(|step| match step {
                Step::State(state) => state_word(*state),
                Step::Behavior(name) => name.as_str(),
            })
            .collect();
        lines.push_str(&format!("lately: {}\n", words.join(" → ")));
    }
    (streak, was, lines)
}

fn doing_words(doing: &Doing) -> String {
    match &doing.behavior {
        Some(name) => format!("{}, doing {name}", state_word(doing.state)),
        None => state_word(doing.state).to_string(),
    }
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
