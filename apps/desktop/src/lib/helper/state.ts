import { writable } from "svelte/store";
import type { HelperStatus } from "../ipc/helper";
export const helper = writable<HelperStatus | null>(null);

export const proposalPreview = writable<import("../ipc/helper").Proposal | null>(null);

export const codexStatus = writable<import("../ipc/helper").CodexStatus | null>(null);
