export interface PedalFrame {
  /** Monotonic timestamp in microseconds (µs) */
  t: number;
  /** Brake position normalized to 0..1 */
  brake: number;
  /** Throttle position normalized to 0..1 */
  throttle: number;
  /** Optional clutch position normalized to 0..1 */
  clutch?: number;
}
