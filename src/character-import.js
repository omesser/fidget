// A user-facing Character ZIP installer inside the existing Character tab.
// The Rust commands own all filesystem paths and validation; the webview never
// receives read/write access to arbitrary files.

export function importMessage(name) {
  return `${name} installed. Restart Fidget to use it.`;
}

export function removeMessage(name) {
  return `${name} removed. Restart Fidget to update the Character menu.`;
}

export function mountCharacterImports(panel) {
  const section = panel.querySelector(".set-section");
  if (!section) return;

  const wrap = document.createElement("div");
  wrap.className = "set-character-imports";
  const heading = document.createElement("h3");
  heading.textContent = "Character Imports";
  const intro = document.createElement("p");
  intro.className = "set-help";
  intro.textContent = "Add a Character from a ZIP file.";
  const warning = document.createElement("p");
  warning.className = "set-notice set-caution";
  warning.textContent = "Characters include a personality the AI reads. Only import ones from people you trust.";
  const importButton = document.createElement("button");
  importButton.type = "button";
  importButton.textContent = "Import from zip";
  // .set-controls gives it the same size as every other Settings button.
  const controls = document.createElement("div");
  controls.className = "set-controls";
  controls.append(importButton);
  const message = document.createElement("p");
  message.className = "set-status";
  message.setAttribute("role", "status");
  message.setAttribute("aria-live", "polite");
  const list = document.createElement("ul");
  list.className = "set-list";
  wrap.append(heading, intro, controls, warning, message, list);
  section.append(wrap);

  const busy = (on) => { importButton.disabled = on; };
  // The Shell returns bare errors; this is the one place that labels them.
  const showError = (error) => {
    const detail = typeof error === "string" ? error : (error?.message || String(error));
    message.textContent = `Character Imports: ${detail}`;
  };

  async function refresh() {
    try {
      const characters = await window.__TAURI_INTERNALS__.invoke("character_import_list");
      if (!wrap.isConnected) return;
      list.replaceChildren();
      if (characters.length === 0) {
        const empty = document.createElement("li");
        empty.textContent = "No imported Character ZIPs yet.";
        list.append(empty);
      }
      for (const character of characters) {
        const item = document.createElement("li");
        const label = document.createElement("span");
        label.textContent = character.name;
        const remove = document.createElement("button");
        remove.type = "button";
        remove.textContent = "Remove";
        remove.disabled = !character.removable;
        if (!character.removable) remove.title = "Selected or running; switch Character and restart Fidget first";
        remove.addEventListener("click", async () => {
          remove.disabled = true;
          try {
            await window.__TAURI_INTERNALS__.invoke("character_import_remove", { id: character.id });
            message.textContent = removeMessage(character.name);
          } catch (error) {
            showError(error);
          }
          await refresh();
        });
        item.append(label, remove);
        list.append(item);
      }
    } catch (error) {
      if (wrap.isConnected) showError(error);
    }
  }

  importButton.addEventListener("click", async () => {
    busy(true);
    try {
      const name = await window.__TAURI_INTERNALS__.invoke("character_import_zip");
      if (name) {
        message.textContent = importMessage(name);
        await refresh();
      }
    } catch (error) {
      showError(error);
    } finally {
      busy(false);
    }
  });
  refresh();
}
