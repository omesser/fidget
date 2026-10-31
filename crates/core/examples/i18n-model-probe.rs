//! Print the same app-level language contract used by Fidget's Director.
//! Usage: cargo run -q -p fidget-core --example i18n-model-probe -- fr
use fidget_core::director::app_instructions;

fn main() {
    let code = std::env::args().nth(1).expect("language: en, fr or es");
    let language = match code.as_str() {
        "en" => None,
        "fr" => Some("French"),
        "es" => Some("Spanish"),
        _ => panic!("unknown language code: {code}"),
    };
    let rules = app_instructions(["wave", "nap", "sit"], false, language);
    // The same opening-turn structure as WakeRequest::text(false):
    // authored persona, app-level rules, instance id and current moment.
    // The persona and moment are explicit neutral test fixtures.
    print!(
        "You are BMO, a friendly animated desk companion.\n\n{rules}\n\n\
         Your instance_id is bmo-1. Pass it to every Fidget tool you call.\n\n\
         what just happened: spoken to\n\
         recent: (none)\n\
         time: Saturday 12:00\n\
         state: idle\n\
         standing on: nothing\n\
         they said: Please speak to me now. Answer my question in two complete cheerful sentences: what would you like to do today? Include the sentences in the spoken reply, not just a behavior name."
    );
}
