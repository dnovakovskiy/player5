// WebMidiClock: MIDI clock in from a Web MIDI input (Chromium; a DJM mixer
// over USB, a controller, another app). Forwards the System Real-Time
// clock messages with their event timestamps (performance clock); the
// core's MIDI follower turns them into tempo and phase. See
// docs/protocols/midi-clock.md.

/** core/ffi MIDI codes for the status bytes we forward. */
const STATUS_CODE: Record<number, number> = { 0xf8: 0, 0xfa: 1, 0xfb: 2, 0xfc: 3 };

export interface MidiPort {
  id: string;
  name: string;
}

export interface MidiEvents {
  /** A clock message (core code) received at performance time `ms`. */
  onClock(code: number, ms: number): void;
  onInputs(inputs: MidiPort[]): void;
}

interface MinimalInput {
  id: string;
  name?: string | null;
  manufacturer?: string | null;
  state?: string;
  onmidimessage: ((e: { data: Uint8Array | null; timeStamp: number }) => void) | null;
}

interface MinimalAccess {
  inputs: { forEach(cb: (input: MinimalInput) => void): void };
  onstatechange: (() => void) | null;
}

export function midiSupported(): boolean {
  return typeof navigator !== "undefined" && typeof navigator.requestMIDIAccess === "function";
}

export class MidiClockInput {
  private access: MinimalAccess | null = null;
  private current: MinimalInput | null = null;
  selectedId: string | null = null;
  clocks = 0;

  constructor(private readonly events: MidiEvents) {}

  async open(): Promise<MidiPort[]> {
    if (!this.access) {
      this.access = (await navigator.requestMIDIAccess({ sysex: false })) as unknown as MinimalAccess;
      this.access.onstatechange = () => {
        const list = this.inputs();
        this.events.onInputs(list);
        if (this.selectedId && !list.some((p) => p.id === this.selectedId)) this.attach(null);
        else if (this.selectedId && !this.current) this.attach(this.selectedId);
      };
    }
    const list = this.inputs();
    this.events.onInputs(list);
    return list;
  }

  inputs(): MidiPort[] {
    const out: MidiPort[] = [];
    this.access?.inputs.forEach((input) => {
      if (input.state === "disconnected") return;
      const name = [input.manufacturer, input.name].filter(Boolean).join(" ").trim();
      out.push({ id: input.id, name: name || input.id });
    });
    return out;
  }

  /** Listens to one input (null: none). */
  select(id: string | null): void {
    this.selectedId = id;
    this.attach(id);
  }

  private attach(id: string | null): void {
    if (this.current) this.current.onmidimessage = null;
    this.current = null;
    if (!id || !this.access) return;
    let found: MinimalInput | null = null;
    this.access.inputs.forEach((input) => {
      if (input.id === id) found = input;
    });
    this.current = found;
    const current = found as MinimalInput | null;
    if (current) {
      current.onmidimessage = (e) => {
        const status = e.data?.[0];
        if (status === undefined) return;
        const code = STATUS_CODE[status];
        if (code === undefined) return;
        if (code === 0) this.clocks++;
        this.events.onClock(code, e.timeStamp);
      };
    }
  }

  close(): void {
    this.attach(null);
  }
}
