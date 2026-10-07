import type { NotificationSound } from "./wire.ts";

export type { NotificationSound } from "./wire.ts";

const TONES: Record<Exclude<NotificationSound, "system">, [number, number][]> = {
  chime: [[660, 0], [880, 0.13]],
  pop: [[740, 0]],
  soft: [[520, 0]],
};

let context: AudioContext | undefined;

async function resumeAudioContext(audio: AudioContext): Promise<boolean> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      audio.resume(),
      new Promise<void>((resolve) => { timer = setTimeout(resolve, 750); }),
    ]);
    return audio.state === "running";
  } catch {
    return false;
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

export async function playNotificationSound(sound: NotificationSound, current: () => boolean = () => true): Promise<boolean> {
  if (sound === "system" || !current() || typeof window === "undefined" || !("AudioContext" in window)) return false;
  try {
    context ??= new AudioContext();
    if (context.state !== "running" && !await resumeAudioContext(context)) return false;
    if (!current() || context.state !== "running") return false;
    const now = context.currentTime;
    const waveform: OscillatorType = sound === "soft" ? "triangle" : "sine";
    const volume = sound === "pop" ? 0.16 : 0.12;
    for (const [frequency, offset] of TONES[sound]) {
      const oscillator = context.createOscillator();
      const gain = context.createGain();
      const start = now + offset;
      oscillator.type = waveform;
      oscillator.frequency.value = frequency;
      gain.gain.setValueAtTime(0, start);
      gain.gain.linearRampToValueAtTime(volume, start + 0.015);
      gain.gain.exponentialRampToValueAtTime(0.001, start + (sound === "pop" ? 0.11 : 0.2));
      oscillator.connect(gain).connect(context.destination);
      oscillator.start(start);
      oscillator.stop(start + (sound === "pop" ? 0.12 : 0.22));
    }
    return true;
  } catch {
    return false;
  }
}
