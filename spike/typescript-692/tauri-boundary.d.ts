// Spike 692, option 2. Types `window.__TAURI__` for the two fidget webviews
// so that every `listen(name, ({ payload }) => …)` and `invoke(name, args)`
// in src/ is checked against the Rust payloads — with no edit to src/.
// Payload shapes come from ts-rs-proto/generated/payloads.d.ts, which
// `cargo test` regenerates from the Rust structs.
import type {
  ArtUrls, ChatOpening, ChatReply, ChatStatusPush, PermissionAsk, Placement, Settled,
} from "./ts-rs-proto/generated/payloads";

// Event name -> payload. main.rs:113-153 owns the names.
interface Events {
  frame: Placement;
  chat: ChatReply;
  "chat-status": ChatStatusPush;
  "chat-opening": ChatOpening;
  "chat-session": string;
  "chat-permission": PermissionAsk;
  "chat-thought": string;
  "chat-permission-settled": Settled;
}

// Command name -> [args, result]. main.rs generate_handler! owns the list.
interface Commands {
  character: [undefined, ArtUrls];
  overlay_primary: [{ down: boolean }, void];
  overlay_secondary: [{ down: boolean }, void];
  overlay_hotspots: [{ rects: number[][] }, void];
  overlay_hit_tests_hotspots: [undefined, boolean];
  overlay_open_chat: [{ id: string }, void];
  chat_opening: [{ instance: string }, ChatOpening];
  chat_send: [{ instance: string; text: string }, void];
  chat_prompt: [{ instance: string; text: string }, void];
  chat_ready: [{ instance: string }, void];
  permission_answer: [{ request: string; option: string }, void];
  open_link: [{ url: string }, void];
  select_harness: [{ harness: string }, string];
  show_settings: [undefined, void];
  show_ai_source: [undefined, void];
  settings_snapshot: [undefined, unknown];
}

declare global {
  interface Window {
    __TAURI__: {
      core: {
        invoke<K extends keyof Commands>(cmd: K, args?: Commands[K][0]): Promise<Commands[K][1]>;
      };
      event: {
        listen<K extends keyof Events>(
          name: K,
          handler: (event: { payload: Events[K] }) => void,
          options?: { target?: unknown },
        ): Promise<() => void>;
      };
      webviewWindow: { getCurrentWebviewWindow(): any };
    };
  }
}
export {};
