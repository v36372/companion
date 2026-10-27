// agent/src/credentials.ts
import type { Credential, CredentialInfo, CredentialStore } from "@earendil-works/pi-ai";

const DUMMY = "guest-dummy-not-a-secret";
const PROVIDERS = ["openai", "anthropic", "openrouter"] as const;

function dummyApiKey(): Credential {
  return { type: "api_key", key: DUMMY };
}

/** pi-ai 1.4.0 CredentialStore. There is no getApiKey. Models.getAuth() calls read(). */
export const dummyCredentials: CredentialStore = {
  async read(providerId: string): Promise<Credential | undefined> {
    if (!(PROVIDERS as readonly string[]).includes(providerId)) return undefined;
    return dummyApiKey();
  },
  async list(): Promise<readonly CredentialInfo[]> {
    return PROVIDERS.map((providerId) => ({ providerId, type: "api_key" as const }));
  },
  async modify(
    providerId: string,
    _fn: (current: Credential | undefined) => Promise<Credential | undefined>,
    _options?: unknown,
  ): Promise<Credential | undefined> {
    return this.read(providerId);
  },
  async delete(_providerId: string): Promise<void> {
    // Keep the dummy so getAuth() still succeeds. Host overwrite is the only strip.
  },
};
