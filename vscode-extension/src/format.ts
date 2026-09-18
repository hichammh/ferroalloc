/**
 * Byte formatting shared by every view (CodeLens, status bar, diff).
 *
 * Handles negative values: memory deltas go both ways, and testing `bytes < 1024`
 * on a signed value caught every negative number in the "bytes" branch, printing
 * a 3 MB drop as `-3145728 B`.
 */
export function formatBytes(bytes: number): string {
    const sign = bytes < 0 ? '-' : '';
    const abs = Math.abs(bytes);
    if (abs < 1024) { return `${sign}${abs} B`; }
    if (abs < 1024 * 1024) { return `${sign}${(abs / 1024).toFixed(1)} KB`; }
    return `${sign}${(abs / (1024 * 1024)).toFixed(2)} MB`;
}
