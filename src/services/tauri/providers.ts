import { invoke } from "@tauri-apps/api/core";

// Mirrors commands/providers.rs. The key is write-only: it goes in once and only `present` comes back.

export interface CredentialStatusDto {
  present: boolean;
}

// Thrown (rejected) by invoke. Fixed categories, never any text from the key or the OS store.
export type CredentialErrorCategory =
  | "unsupportedProvider"
  | "unsupportedKind"
  | "invalidValue"
  | "storeUnavailable"
  | "storeAccess";

export const CREDENTIAL_ERROR_MESSAGES: Record<CredentialErrorCategory | "unknown", string> = {
  invalidValue: "That is not a valid API key. Keys are 20 to 128 letters, digits, dashes or underscores.",
  storeUnavailable: "The system key store is not available, so the key was not saved.",
  storeAccess: "The system key store refused access. Unlock it and try again.",
  unsupportedProvider: "This provider does not take an API key.",
  unsupportedKind: "This provider does not take an API key.",
  unknown: "Something went wrong. Try again.",
};

export function credentialErrorMessage(e: unknown): string {
  const c = typeof e === "object" && e !== null && "category" in e ? String(e.category) : "";
  return Object.hasOwn(CREDENTIAL_ERROR_MESSAGES, c)
    ? CREDENTIAL_ERROR_MESSAGES[c as CredentialErrorCategory]
    : CREDENTIAL_ERROR_MESSAGES.unknown;
}

export function providerCredentialSet(provider: string, kind: "apiKey", value: string): Promise<void> {
  return invoke<void>("provider_credential_set", { args: { provider, kind, value } });
}

export function providerCredentialStatus(provider: string): Promise<CredentialStatusDto> {
  return invoke<CredentialStatusDto>("provider_credential_status", { args: { provider } });
}

export function providerCredentialClear(provider: string): Promise<void> {
  return invoke<void>("provider_credential_clear", { args: { provider } });
}
