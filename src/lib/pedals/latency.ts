/**
 * Creates a rising-edge detector with hysteresis.
 *
 * @param threshold Value that triggers the rising edge event (default: 0.5)
 * @param rearm Value below which the detector re-arms for the next trigger (default: 0.4)
 * @returns A function taking the current value and returning true exactly once per crossing
 */
export function createCrossingDetector(threshold = 0.5, rearm = 0.4): (value: number) => boolean {
  if (rearm >= threshold) {
    throw new Error(
      `Rearm threshold (${rearm}) must be strictly less than trigger threshold (${threshold})`,
    );
  }

  let armed = true;

  return (value: number): boolean => {
    if (armed) {
      if (value >= threshold) {
        armed = false;
        return true;
      }
    } else {
      if (value <= rearm) {
        armed = true;
      }
    }
    return false;
  };
}
