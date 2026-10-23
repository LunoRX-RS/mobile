import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Mirrors commands/remote.rs. The Rust side owns the WebSocket and the session token; the webview only sends and
// receives JSON text frames (backend docs/remote-control.md).

export type LinkStatus = "connecting" | "online" | "offline" | "signedOut";

export const remoteStart = (deviceId: string, name: string) => invoke<void>("remote_start", { deviceId, name });
export const remoteStop = () => invoke<void>("remote_stop");
export const remoteSend = (message: object) => invoke<void>("remote_send", { message });

export const subscribeRemoteMessages = (onText: (text: string) => void): Promise<UnlistenFn> =>
  listen<string>("remote://message", (e) => onText(e.payload));
export const subscribeRemoteStatus = (onStatus: (status: LinkStatus) => void): Promise<UnlistenFn> =>
  listen<LinkStatus>("remote://status", (e) => onStatus(e.payload));
