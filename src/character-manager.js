// A user-facing Character ZIP installer inside the existing Character tab.
// The Rust commands own all filesystem paths and validation; the webview never
// receives read/write access to arbitrary files.

export function importMessage(name) {
  return `${name} installed.`;
}

export function removeMessage(name) {
  return `${name} removed. Restart Fidget to update the Character menu.`;
}

export function mountCharacterManager(panel) {
  const section = panel.querySelector(".set-section");
  if (!section) return;

  const wrap = document.createElement("div");
  wrap.className = "set-character-manager";
  const heading = document.createElement("h3");
  heading.textContent = "Character Manager";
  const intro = document.createElement("p");
  intro.className = "set-help";
  intro.textContent = "Install a Character Package from a local ZIP. No extraction or environment variables needed.";
  const warning = document.createElement("p");
  warning.className = "set-help set-character-warning";
  warning.textContent = "Characters include a personality the AI reads. Only import ones from people you trust.";
  const importButton = document.createElement("button");
  importButton.type = "button";
  importButton.textContent = "Import character ZIP…";
  const message = document.createElement("p");
  message.className = "set-status";
  message.setAttribute("role", "status");
  message.setAttribute("aria-live", "polite");
  const list = document.createElement("ul");
  list.className = "set-character-list";
  wrap.append(heading, intro, warning, importButton, message, list);
  section.append(wrap);

  const invoke = (command, args = {}) => window.__TAURI_INTERNALS__.invoke(command, args);
  const busy = (on) => { importButton.disabled = on; };
  const showError = (error) => {
    const detail = typeof error === "string" ? error : (error?.message || String(error));
    message.textContent = `Character Manager: ${detail}`;
  };

  async function refresh() {
    try {
      const characters = await invoke("character_manager_list");
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
          if (!window.confirm(`Remove ${character.name} from Fidget?`)) return;
          remove.disabled = true;
          try {
            await invoke("character_manager_remove", { id: character.id });
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
    if (!window.confirm("Characters include a personality the AI reads. Only import ones from people you trust. Continue?")) return;
    busy(true);
    try {
      const name = await invoke("character_manager_import");
      if (name) {
        message.textContent = importMessage(name);
        window.alert("Restart Fidget to use it.");
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
