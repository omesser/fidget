# Live model language proof

**Model:** `qwen2.5:7b` (local Ollama, GitHub-hosted Ubuntu runner).
**Method:** The Rust helper calls Fidget's actual `app_instructions()`.
The same OpenAI-compatible chat endpoint as Fidget's Model API receives
a BMO character test turn. The unmodified model reply is recorded;
the spoken portion is checked with `langdetect` (confidence >= 0.80).

No canned speech, no secret API key and no translated test answers.
This verifies this model and these turns, not a universal guarantee
for every model, prompt or future response.

| Selected language | Detected | Confidence | Result | Actual spoken response |
| --- | --- | ---: | --- | --- |
| English | en | 1.000 | PASS | What's up, friend? Let's have some fun today! What would you like to do? |
| French | fr | 1.000 | PASS | "Bonjour! Je suis ravi de parler avec toi. Que dirais-tu de faire une petite sieste aujourd'hui?" |
| Spanish | es | 1.000 | PASS | ¿Qué tal un siesta? Me siento un poco cansado hoy, bmo-1. |

The full request and unedited responses are in [results.json](results.json).
The real app and picker screenshots are in the
[language gallery](../gallery/README.md). The French BMO bubble there
is a deterministic ACP fixture and is **not** this live-model test.

**Reproduce:** [i18n-model-proof GitHub Actions workflow](../../../.github/workflows/i18n-model-proof.yml).
