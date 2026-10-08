/**
 * Convert a raw axis value from signed 16-bit to unsigned 0..65535 for display.
 * SDL3 reports axes as signed 16-bit (-32768..32767), but Windows and vendor tools
 * show them as unsigned (0..65535).
 */
export const rawDisplay = (raw: number): number => raw + 32768;
