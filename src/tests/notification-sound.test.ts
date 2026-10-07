import assert from "node:assert/strict";
import test from "node:test";

import { playNotificationSound } from "../lib/utils/notification-sound.ts";

test("notification sound preview synthesizes selected tones without notifications", async () => {
  const oscillators: { frequency: number; type: string; starts: number[]; stops: number[] }[] = [];
  let fakeContext!: FakeAudioContext;
  let resolveResume!: () => void;
  let resumeMode: "pending" | "resolved" = "pending";
  class FakeAudioContext {
    currentTime = 1;
    state = "suspended";
    destination = {};
    constructor() { fakeContext = this; }
    resume() {
      if (resumeMode === "resolved") return Promise.resolve();
      return new Promise<void>((resolve) => { resolveResume = resolve; });
    }
    createOscillator() {
      const oscillator = {
        type: "sine",
        frequency: { value: 0 },
        starts: [] as number[],
        stops: [] as number[],
        connect: () => gain,
        start: (time: number) => oscillator.starts.push(time),
        stop: (time: number) => oscillator.stops.push(time),
      };
      const gain = { gain: { setValueAtTime() {}, linearRampToValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: () => ({}) };
      Object.defineProperty(oscillator.frequency, "value", { set: (frequency: number) => oscillators.push({ frequency, type: oscillator.type, starts: oscillator.starts, stops: oscillator.stops }) });
      return oscillator;
    }
    createGain() {
      return { gain: { setValueAtTime() {}, linearRampToValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: () => ({}) };
    }
  }
  Object.defineProperty(globalThis, "window", { configurable: true, value: { AudioContext: FakeAudioContext } });
  Object.defineProperty(globalThis, "AudioContext", { configurable: true, value: FakeAudioContext });
  assert.equal(await playNotificationSound("system"), false);
  let current = true;
  assert.equal(await playNotificationSound("chime", () => current), false);
  assert.equal(oscillators.length, 0);

  const pending = playNotificationSound("chime", () => current);
  await new Promise<void>((resolve) => setImmediate(resolve));
  current = false;
  resolveResume();
  assert.equal(await pending, false);
  assert.equal(oscillators.length, 0);

  current = true;
  resumeMode = "resolved";
  assert.equal(await playNotificationSound("chime", () => current), false);
  assert.equal(oscillators.length, 0);

  fakeContext.state = "running";
  assert.equal(await playNotificationSound("chime", () => current), true);
  assert.equal(await playNotificationSound("pop"), true);
  assert.deepEqual(oscillators.map((tone) => tone.frequency), [660, 880, 740]);
  assert.equal(oscillators[0].starts[0], 1);
  assert.equal(oscillators[1].starts[0], 1.13);
});
