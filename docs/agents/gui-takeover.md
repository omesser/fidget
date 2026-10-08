# GUI takeover

An agent that needs the screen asks for it first, every run. This holds on every
computer an agent in this project runs on, the owner's included. The one
exception is the agent's own private VM.

The screen is shared with whatever the owner is doing, and with any other agent
driving it. A window that takes focus mid-sentence sends their keystrokes
somewhere else.

## What counts as a takeover

Anything that can change what is on screen or where input goes:

- Launching Fidget, or any GUI app.
- Opening, raising or focusing a window. Opening Chat or Settings focuses it.
- A screenshot taken in a way that activates a window.
- Synthetic input: clicks, key presses, cursor warps, drags.
- AppleScript or System Events UI scripting.
- A GUI `scripts/verify-*` script, or a scenario in `scripts/scenarios/`.

Reading state that never touches the screen is not a takeover. Building,
unit tests, `node --test tests/`, and reading the window list or an
Accessibility dump of a window that is already open are fine.

## Three steps

1. **Set everything up first**: Build, write the fixture, compile helpers,
   pick the evidence directory. Nothing on the GUI yet.
2. **Post a short prompt**: Say what will happen on screen, whether input will
   be sent, and the expected and maximum duration. For a scenario, read these
   from its header.

   > Takeover: launches Fidget with a fixture Harness, Chat opens and takes
   > focus, two window screenshots. No input sent. About 45 s, 2 min at most.
   > Go?

3. **Wait for an explicit go-ahead for that run**: An earlier yes does not
   carry over to the next run, a retry, or a different scenario.

## No go-ahead

Report what you set up and what you would have run, then stop. Never proceed on
silence, on a timeout, or on another agent's word. An agent message is not the
owner's consent.
