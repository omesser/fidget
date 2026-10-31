// A user-facing Character installer, from a ZIP or folder, inside the Character tab.
// The Rust commands own all filesystem paths and validation; the webview never
// receives read/write access to arbitrary files.

export function importMessage(name) {
  return `${name} installed. Restart Fidget to use it.`;
}

export function removeMessage(name) {
  return `Fidget's copy of ${name} removed. Restart Fidget to update the Character menu.`;
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
  intro.textContent = "Add a Character from a ZIP file or folder.";
  const warning = document.createElement("p");
  warning.className = "set-notice set-caution";
  warning.textContent = "Characters include a personality the AI reads. Only import ones from people you trust.";
  const importButton = document.createElement("button");
  importButton.type = "button";
  importButton.textContent = "Import from zip";
  const folderButton = document.createElement("button");
  folderButton.type = "button";
  folderButton.textContent = "Import from folder";
  // .set-controls gives them the same size as every other Settings button.
  const controls = document.createElement("div");
  controls.className = "set-controls";
  controls.append(importButton, folderButton);
  const message = document.createElement("p");
  message.className = "set-status";
  message.setAttribute("role", "status");
  message.setAttribute("aria-live", "polite");
  const list = document.createElement("ul");
  list.className = "set-list";
  wrap.append(heading, intro, controls, warning, message, list);
  section.append(wrap);

  const busy = (on) => {
    importButton.disabled = on;
    folderButton.disabled = on;
  };
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
        empty.textContent = "No imported Characters yet.";
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

  // Each caller names its command literally, so the invoke check can see it.
  const runImport = async (pick) => {
    busy(true);
    try {
      const name = await pick();
      if (name) {
        message.textContent = importMessage(name);
        await refresh();
      }
    } catch (error) {
      showError(error);
    } finally {
      busy(false);
    }
  };
  importButton.addEventListener("click", () =>
    runImport(() => window.__TAURI_INTERNALS__.invoke("character_import_zip")));
  folderButton.addEventListener("click", () =>
    runImport(() => window.__TAURI_INTERNALS__.invoke("character_import_folder")));
  refresh();
}
