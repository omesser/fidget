// Hand-written snapshot of `form::describe().tabs["AI"]` plus the slice of
// `SettingsView` the tab reads, in two states. Copy is lifted verbatim from
// src-tauri/src/settings/form.rs (director_sections, completer_source_section).
// Shape mirrors serde's externally-tagged enum: {type: "Checkbox", ...}.

const ENDPOINTS = [
  "Custom",
  "Ollama (http://localhost:11434)",
  "LM Studio (http://localhost:1234)",
  "oMLX (http://localhost:8000)",
  "llama.cpp (http://localhost:8080)",
  "OpenAI (https://api.openai.com)",
  "Anthropic (https://api.anthropic.com)",
];

const HARNESSES = [
  "Model API",
  "Harness · claude",
  "Harness · codex",
  "Harness · grok",
  "Harness · hermes",
  "Harness · opencode",
  "Harness · pi",
  "Harness · Custom",
];

const HTTP_FROZEN = "Not in use: a Harness is the Model API";

function aiTab({ driving }) {
  const httpStatus = driving ? HTTP_FROZEN : null;
  return {
    title: "AI",
    sections: [
      {
        heading: "AI",
        comment:
          "Control whether the fidget improvises, and how often it starts a conversation on its own.",
        disclosure:
          "The fidget can run on static weights (no model calls) or with a Model API (the HTTP endpoint below, or an attached Harness). AI on with no Harness uses the HTTP endpoint. An attached Harness that answers becomes the \"AI brain\".",
        status: null,
        rows: [
          {
            type: "Checkbox",
            id: "director",
            label: "AI on",
            frozen: false,
            help: "Lets the model pick what happens next.",
            disclosure:
              "With this off, the fidget runs on static weights: predefined behaviors chosen by their declared weights, no model involved. With it on and no Harness attached, the HTTP endpoint (base URL, model, and key below) proposes behaviors and short lines. With it on and a Harness attached that answers, that Harness is the \"AI brain\" for every Instance.",
            status: null,
          },
          {
            type: "Checkbox",
            id: "ambient",
            label: "Ambient session wakes",
            frozen: false,
            help: "Acts on its own, not only when asked.",
            disclosure:
              "Ambient wakes are proactive model calls: the fidget addresses you after being idle long enough, on an exponential backoff. With this off, wakes are reactive only — you have to address it first. The switch below sets how long the first ambient wake waits.",
            status: null,
          },
          {
            type: "TextField",
            id: "director_wake_secs",
            label: "First wake, in seconds",
            placeholder: "90",
            frozen: false,
            batched: false,
            help: null,
            disclosure: null,
            status: null,
          },
        ],
      },
      {
        heading: "AI source",
        comment: "Choose which \"AI brain\" answers: Model API or an attached Harness.",
        disclosure:
          "Model API uses the HTTP endpoint below (base URL, model, and key). A Harness (claude, codex, grok, hermes, opencode, or Custom) attaches a child process and makes it the AI brain, and the HTTP rows stop driving. Every pick takes effect now: Model API leaves the HTTP endpoint, and a Harness is attached at once, answering once its child is up.",
        status: null,
        rows: [
          {
            type: "Popup",
            id: "harness",
            label: "AI source",
            help: "Which \"AI brain\" answers for the fidget.",
            options: HARNESSES,
            frozen: false,
            disclosure:
              "Model API: the HTTP endpoint below. Harness · {name}: starts that Harness and makes it the AI brain. Harness · Custom: the command line below. The line below this row shows what is attached and whether it is signed in.",
            status: null,
          },
          {
            type: "TextField",
            id: "harness_command",
            label: "Custom command line",
            placeholder: "opencode acp",
            frozen: false,
            batched: false,
            help: null,
            disclosure:
              "The command Fidget runs when Custom is picked above. Blur commits it and re-opens the attachment.",
            status: null,
          },
          {
            type: "InspectBlock",
            id: "harness_state",
            label: null,
            help: "Harness signs itself in - Fidget never asks for credentials.",
            disclosure:
              "Fidget holds no credential for the Harness. The Harness authenticates itself, and the login command this line may show is text: nothing here runs it for you. This line shows three states: not attached, attached but not signed in (with the login command), or attached and answering (with a session UUID).",
            status: null,
          },
        ],
      },
      {
        heading: "Model / API",
        comment: null,
        disclosure: null,
        status: null,
        rows: [
          {
            type: "Composite",
            id: "base_url_pick",
            help: driving
              ? "Off, for the same reason the Base URL below is."
              : "Fills in the Base URL below. Any other OpenAI-compatible endpoint can be typed there.",
            disclosure: null,
            controls: [
              {
                type: "Popup",
                id: "director_base_url_pick",
                options: ENDPOINTS,
                frozen: driving,
                fills: { row: "director_base_url" },
              },
            ],
          },
          {
            type: "TextField",
            id: "director_base_url",
            label: "Base URL",
            placeholder: "https://api.openai.com",
            frozen: driving,
            batched: true,
            help: null,
            disclosure: null,
            status: httpStatus,
          },
          {
            type: "TextField",
            id: "director_model",
            label: "Model",
            placeholder: "gpt-4o-mini",
            frozen: driving,
            batched: true,
            help: null,
            disclosure: null,
            status: httpStatus,
          },
          {
            type: "SecureField",
            id: "director_api_key",
            label: "API key",
            frozen: driving,
            status: httpStatus,
          },
          {
            type: "Composite",
            id: "api_key_actions",
            help: null,
            disclosure: null,
            controls: [{ type: "Button", id: "clear_key", label: "Clear key", frozen: driving }],
          },
          {
            type: "Composite",
            id: "director_actions",
            help: "The endpoint rows take effect on Apply.",
            disclosure: null,
            controls: [
              { type: "Button", id: "director_apply", label: "Apply", frozen: false },
              { type: "Button", id: "director_cancel", label: "Cancel", frozen: false },
            ],
          },
        ],
      },
      {
        heading: "Last user turn",
        comment: null,
        disclosure: null,
        status: null,
        rows: [
          {
            type: "InspectBlock",
            id: "payload",
            label: null,
            help: "The last thing sent to the model.",
            disclosure:
              "This is the Character Prompt (opening) and the last message, exactly as sent to the model.",
            status: null,
          },
        ],
      },
    ],
  };
}

// The slice of SettingsView the AI tab draws, per state.
export const STATES = {
  modelApi: {
    label: "Model API drives",
    tab: aiTab({ driving: false }),
    view: {
      director: true,
      ambient: true,
      director_wake_secs: "",
      harness: "Model API",
      harness_command: "",
      harness_state: "Not attached.",
      director_base_url: "http://localhost:11434",
      director_model: "qwen2.5:7b",
      api_key_placeholder: "Not set", // pragma: allowlist secret
      payload: "opening: You are BMO, a small green console…\nlast: (poke) — the user tapped you",
    },
  },
  harnessDriving: {
    label: "A Harness drives",
    tab: aiTab({ driving: true }),
    view: {
      director: true,
      ambient: false,
      director_wake_secs: "120",
      harness: "Harness · claude",
      harness_command: "",
      harness_state: "claude · signed in · session 3f9c2a1e-7b44-4d0e-9a11-0c6f2e5d8b77",
      director_base_url: "http://localhost:11434",
      director_model: "qwen2.5:7b",
      api_key_placeholder: "Set (…4f2a)", // pragma: allowlist secret
      payload: "opening: You are BMO, a small green console…\nlast: (summon) hi there",
    },
  },
};
