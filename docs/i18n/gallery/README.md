# Fidget language gallery

Real screenshots captured from the Fidget binary running on a GitHub-hosted
Ubuntu Linux runner, in an isolated X11/Xvfb desktop with GTK/WebKit.
These images are **not mockups**. The workflow opens Settings and Chat,
switches the Language picker while Chat stays open, visits every tab,
and opens the context menu. No user profile or credentials are used.

**Reproduce:** [i18n-gallery GitHub Actions workflow](../../../.github/workflows/i18n-gallery.yml).

| Surface | English | Français | Español |
| --- | --- | --- | --- |
| **Presence** | ![Presence — English](en/01-presence.png) | ![Presence — Français](fr/01-presence.png) | ![Presence — Español](es/01-presence.png) |
| **Character** | ![Character — English](en/02-character.png) | ![Character — Français](fr/02-character.png) | ![Character — Español](es/02-character.png) |
| **AI** | ![AI — English](en/03-ai.png) | ![AI — Français](fr/03-ai.png) | ![AI — Español](es/03-ai.png) |
| **Chat** | ![Chat — English](en/04-chat.png) | ![Chat — Français](fr/04-chat.png) | ![Chat — Español](es/04-chat.png) |
| **Privacy** | ![Privacy — English](en/05-privacy.png) | ![Privacy — Français](fr/05-privacy.png) | ![Privacy — Español](es/05-privacy.png) |
| **Caffeinate** | ![Caffeinate — English](en/06-caffeinate.png) | ![Caffeinate — Français](fr/06-caffeinate.png) | ![Caffeinate — Español](es/06-caffeinate.png) |
| **Development** | ![Development — English](en/07-development.png) | ![Development — Français](fr/07-development.png) | ![Development — Español](es/07-development.png) |
| **Language picker (expanded)** | ![Language picker (expanded) — English](en/08-language-picker.png) | ![Language picker (expanded) — Français](fr/08-language-picker.png) | ![Language picker (expanded) — Español](es/08-language-picker.png) |
| **Chat (live language change)** | ![Chat (live language change) — English](en/09-chat.png) | ![Chat (live language change) — Français](fr/09-chat.png) | ![Chat (live language change) — Español](es/09-chat.png) |
| **Context menu** | ![Context menu — English](en/10-context-menu.png) | ![Context menu — Français](fr/10-context-menu.png) | ![Context menu — Español](es/10-context-menu.png) |

## BMO speaking French

**Speech screenshot under verification.** The initial frame showed BMO
animating but did not show the French speech bubble. A revised native-runner
capture now requires the frontend's showSpeech event and visible bubble pixels.
The gallery will display the new image only after those checks pass.

The intended fixture dialogue is: **Bonjour ! Je suis BMO. Enchanté de te rencontrer !**

This scripted fixture is separate from the [real-model language proof](../model-proof/README.md),
which checks actual English, French and Spanish inference responses.

The English screenshots use the English source strings; the French
and Spanish screenshots use the branch's translation catalogs.
The language is chosen through the actual in-app picker, not
through a test-only DOM override or command-line language pin.
