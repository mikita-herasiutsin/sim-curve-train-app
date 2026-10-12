import type { PedalFrame } from "./types";

const DEFAULT_CAPACITY = 20000;

/** Reusable output buffers for `FrameHistory.copySince`. */
export interface FrameColumns {
  t: Float64Array;
  brake: Float32Array;
  throttle: Float32Array;
}

/** Allocates buffers that hold `capacity` frames; create once per consumer, not per frame. */
export function createFrameColumns(capacity: number): FrameColumns {
  return {
    t: new Float64Array(capacity),
    brake: new Float32Array(capacity),
    throttle: new Float32Array(capacity),
  };
}

export class FrameHistory {
  readonly capacity: number;
  private readonly tArray: Float64Array;
  private readonly brakeArray: Float32Array;
  private readonly throttleArray: Float32Array;
  private readonly clutchArray: Float32Array;

  private writeIndex = 0;
  private count = 0;

  constructor(capacity = DEFAULT_CAPACITY) {
    if (capacity <= 0 || !Number.isInteger(capacity)) {
      throw new Error(`Capacity must be a positive integer, got ${capacity}`);
    }
    this.capacity = capacity;
    this.tArray = new Float64Array(capacity);
    this.brakeArray = new Float32Array(capacity);
    this.throttleArray = new Float32Array(capacity);
    this.clutchArray = new Float32Array(capacity);
  }

  get length(): number {
    return this.count;
  }

  push(frame: PedalFrame): void {
    const idx = this.writeIndex;
    this.tArray[idx] = frame.t;
    this.brakeArray[idx] = frame.brake;
    this.throttleArray[idx] = frame.throttle;
    this.clutchArray[idx] = frame.clutch !== undefined ? frame.clutch : Number.NaN;

    this.writeIndex = (idx + 1) % this.capacity;
    if (this.count < this.capacity) {
      this.count++;
    }
  }

  pushMany(frames: readonly PedalFrame[]): void {
    const len = frames.length;
    for (let i = 0; i < len; i++) {
      this.push(frames[i]);
    }
  }

  latest(): PedalFrame | undefined {
    if (this.count === 0) {
      return undefined;
    }
    const idx = (this.writeIndex - 1 + this.capacity) % this.capacity;
    return this.getFrameAt(idx);
  }

  clear(): void {
    this.writeIndex = 0;
    this.count = 0;
  }

  private firstIndexSince(tMinUs: number): number {
    const count = this.count;
    if (count === 0) {
      return 0;
    }

    const capacity = this.capacity;
    const oldestIdx = count < capacity ? 0 : this.writeIndex;

    // Binary search over logical index k in [0, count)
    // where logical k maps to physical index (oldestIdx + k) % capacity
    let low = 0;
    let high = count;

    while (low < high) {
      const mid = (low + high) >>> 1;
      const phys = (oldestIdx + mid) % capacity;
      if (this.tArray[phys] < tMinUs) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    return low;
  }

  forEachSince(tMinUs: number, cb: (f: PedalFrame) => void): void {
    const count = this.count;
    if (count === 0) {
      return;
    }

    const low = this.firstIndexSince(tMinUs);
    const capacity = this.capacity;
    const oldestIdx = count < capacity ? 0 : this.writeIndex;

    for (let k = low; k < count; k++) {
      const phys = (oldestIdx + k) % capacity;
      cb(this.getFrameAt(phys));
    }
  }

  /**
   * Copies the frames with t >= tMinUs into `out`, oldest first, and returns how many it copied.
   * Allocates nothing. Throws if any buffer in `out` is shorter than the capacity.
   */
  copySince(tMinUs: number, out: FrameColumns): number {
    const minLength = Math.min(out.t.length, out.brake.length, out.throttle.length);
    if (minLength < this.capacity) {
      throw new Error(
        `Output buffer size ${minLength} is smaller than history capacity ${this.capacity}`,
      );
    }
    const count = this.count;
    if (count === 0) {
      return 0;
    }

    const low = this.firstIndexSince(tMinUs);
    const copiedCount = count - low;
    if (copiedCount <= 0) {
      return 0;
    }

    const capacity = this.capacity;
    const oldestIdx = count < capacity ? 0 : this.writeIndex;
    let outIdx = 0;
    for (let k = low; k < count; k++) {
      const phys = (oldestIdx + k) % capacity;
      out.t[outIdx] = this.tArray[phys];
      out.brake[outIdx] = this.brakeArray[phys];
      out.throttle[outIdx] = this.throttleArray[phys];
      outIdx++;
    }
    return copiedCount;
  }

  getStatsSince(tMinUs: number): { count: number; oldestT: number; newestT: number } | null {
    const count = this.count;
    if (count === 0) {
      return null;
    }

    const low = this.firstIndexSince(tMinUs);
    const matchingCount = count - low;
    if (matchingCount === 0) {
      return null;
    }

    const capacity = this.capacity;
    const oldestIdx = count < capacity ? 0 : this.writeIndex;
    const oldestPhys = (oldestIdx + low) % capacity;
    const newestPhys = (oldestIdx + count - 1) % capacity;
    return {
      count: matchingCount,
      oldestT: this.tArray[oldestPhys],
      newestT: this.tArray[newestPhys],
    };
  }

  private getFrameAt(physicalIdx: number): PedalFrame {
    const clutchVal = this.clutchArray[physicalIdx];
    const frame: PedalFrame = {
      t: this.tArray[physicalIdx],
      brake: this.brakeArray[physicalIdx],
      throttle: this.throttleArray[physicalIdx],
    };
    if (!Number.isNaN(clutchVal)) {
      frame.clutch = clutchVal;
    }
    return frame;
  }
}
