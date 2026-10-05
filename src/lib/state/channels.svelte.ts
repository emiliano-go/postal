import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
import { invoke } from "$lib/utils/ipc";
import type { ChannelPage, ChannelSummary, ChannelView } from "$lib/utils/wire";

function lookupTarget(value: string) {
  const target = value.trim();
  if (/^[^@\s]+@newsletter$/.test(target) || /^[A-Za-z0-9_-]{6,128}$/.test(target)) return target;
  return /^https:\/\/(?:www\.)?whatsapp\.com\/channel\/([A-Za-z0-9_-]{6,128})$/.exec(target)?.[1] ?? null;
}

export class ChannelsState {
  view = $state.raw<ChannelView | null>(null);
  preview = $state.raw<ChannelSummary | null>(null);
  loading = $state(false);
  refreshing = $state(false);
  lookingUp = $state(false);
  busy = $state<string | null>(null);
  error = $state<LocalizedError | null>(null);
  lookupError = $state("");

  private account: string | null = null;
  private generation = -1;
  private viewRequest = 0;
  private lookupRequest = 0;
  private actionRequest = 0;
  private pageRequests = new Map<string, number>();
  private pageCursors = new Map<string, string | null>();
  private pageMore = new Map<string, boolean>();
  private activationKey = "";
  private activationPromise: Promise<void> | null = null;

  private scope(account: string | null, generation: number) {
    if (account === this.account && generation === this.generation) return;
    this.account = account;
    this.generation = generation;
    this.viewRequest++;
    this.lookupRequest++;
    this.actionRequest++;
    this.pageRequests.clear();
    this.pageCursors.clear();
    this.pageMore.clear();
    this.activationKey = "";
    this.activationPromise = null;
    this.view = null;
    this.preview = null;
    this.loading = this.refreshing = this.lookingUp = false;
    this.busy = null;
    this.error = null;
    this.lookupError = "";
  }

  private current(account: string, generation: number) {
    return account === this.account && generation === this.generation;
  }

  reset() {
    this.scope(null, -1);
    this.activationKey = "";
    this.activationPromise = null;
  }

  activate(account: string | null, generation: number, connected: boolean): Promise<void> {
    this.scope(account, generation);
    const key = `${account ?? ""}\0${generation}\0${connected}`;
    if (key === this.activationKey) return this.activationPromise ?? Promise.resolve();
    this.activationKey = key;
    if (!account) return (this.activationPromise = Promise.resolve());
    const activation = (async () => {
      await this.load(account, generation);
      if (connected && this.current(account, generation) && key === this.activationKey) await this.refresh(account, generation);
    })();
    this.activationPromise = activation;
    return activation;
  }

  async load(account: string, generation: number) {
    if (!this.current(account, generation)) return;
    const request = ++this.viewRequest;
    this.loading = true;
    this.error = null;
    try {
      const view = await invoke<ChannelView>("channels", { accountId: account });
      if (this.current(account, generation) && request === this.viewRequest) this.view = view;
    } catch (error) {
      if (this.current(account, generation) && request === this.viewRequest) this.error = normalizeError(error);
    } finally {
      if (this.current(account, generation) && request === this.viewRequest) this.loading = false;
    }
  }

  async refresh(account: string, generation: number) {
    if (!this.current(account, generation)) return;
    const request = ++this.viewRequest;
    this.loading = false;
    this.refreshing = true;
    this.error = null;
    try {
      const view = await invoke<ChannelView>("refresh_channels", { accountId: account });
      if (this.current(account, generation) && request === this.viewRequest) this.view = view;
    } catch (error) {
      if (this.current(account, generation) && request === this.viewRequest) this.error = normalizeError(error);
    } finally {
      if (this.current(account, generation) && request === this.viewRequest) this.refreshing = false;
    }
  }

  async lookup(account: string, generation: number, value: string) {
    if (!this.current(account, generation)) return;
    const jid = lookupTarget(value);
    const request = ++this.lookupRequest;
    this.preview = null;
    this.lookupError = "";
    if (!jid) {
      this.lookupError = "channels.invalid_target";
      return;
    }
    this.lookingUp = true;
    try {
      const channel = await invoke<ChannelSummary>("channel_metadata", { accountId: account, jid });
      if (this.current(account, generation) && request === this.lookupRequest) this.preview = channel;
    } catch (error) {
      if (this.current(account, generation) && request === this.lookupRequest) this.lookupError = normalizeError(error).message;
    } finally {
      if (this.current(account, generation) && request === this.lookupRequest) this.lookingUp = false;
    }
  }

  async follow(account: string, generation: number, jid: string) {
    return this.mutate(account, generation, jid,
      () => invoke<ChannelSummary>("follow_channel", { accountId: account, jid }),
      (channel) => this.update(channel));
  }

  async unfollow(account: string, generation: number, jid: string) {
    return this.mutate(account, generation, jid,
      async () => { await invoke("unfollow_channel", { accountId: account, jid }); return null; },
      () => {
        const current = this.find(jid);
        if (current) this.update({ ...current, followed: false });
        if (this.preview?.jid === jid) this.preview = { ...this.preview, followed: false };
      });
  }

  async setMuted(account: string, generation: number, jid: string, muted: boolean) {
    return this.mutate(account, generation, jid,
      () => invoke<ChannelSummary>("set_channel_muted", { accountId: account, jid, muted }),
      (channel) => this.update(channel));
  }

  async setFavorite(account: string, generation: number, jid: string, favorite: boolean) {
    return this.mutate(account, generation, jid,
      () => invoke<ChannelSummary>("set_channel_favorite", { accountId: account, jid, favorite }),
      (channel) => this.update(channel));
  }

  private async mutate<T>(account: string, generation: number, jid: string, action: () => Promise<T>, apply: (value: T) => void) {
    if (!account || !this.current(account, generation) || this.busy) return false;
    const request = ++this.actionRequest;
    this.busy = jid;
    this.error = null;
    try {
      const value = await action();
      if (!this.current(account, generation) || request !== this.actionRequest) return false;
      apply(value);
      return true;
    } catch (error) {
      if (this.current(account, generation) && request === this.actionRequest) this.error = normalizeError(error);
      return false;
    } finally {
      if (this.current(account, generation) && request === this.actionRequest) this.busy = null;
    }
  }

  private find(jid: string) { return this.view?.channels.find((channel) => channel.jid === jid); }

  private update(channel: ChannelSummary) {
    if (this.view) {
      const found = this.view.channels.some((row) => row.jid === channel.jid);
      this.view = { ...this.view, channels: found
        ? this.view.channels.map((row) => row.jid === channel.jid ? channel : row)
        : [...this.view.channels, channel] };
    }
    if (this.preview?.jid === channel.jid) this.preview = channel;
  }

  hasMoreMessages(jid: string) { return this.pageMore.get(jid) ?? true; }

  async pageMessages(account: string, generation: number, jid: string, limit = 50, latest = false) {
    if (!this.current(account, generation)) return null;
    if (!latest && this.pageMore.get(jid) === false) return null;
    const request = (this.pageRequests.get(jid) ?? 0) + 1;
    this.pageRequests.set(jid, request);
    this.error = null;
    try {
      const page = await invoke<ChannelPage>("channel_messages", {
        accountId: account, jid, before: latest ? null : this.pageCursors.get(jid) ?? null, limit,
      });
      if (!this.current(account, generation) || this.pageRequests.get(jid) !== request) return null;
      this.pageCursors.set(jid, page.next_before);
      this.pageMore.set(jid, page.has_more);
      return page;
    } catch (error) {
      if (this.current(account, generation) && this.pageRequests.get(jid) === request) this.error = normalizeError(error);
      return null;
    }
  }
}

export const channels = new ChannelsState();
