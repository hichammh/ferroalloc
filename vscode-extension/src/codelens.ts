import * as vscode from 'vscode';
import { AnalyzerClient } from './analyzerClient';

/**
 * Provides CodeLens items above Rust functions showing live heap stats:
 *   ⬆ 12 allocs · 48 KB total · 16 KB live
 */
export class MemoryCodeLensProvider implements vscode.CodeLensProvider {
    private emitter = new vscode.EventEmitter<void>();
    readonly onDidChangeCodeLenses = this.emitter.event;

    constructor(private client: AnalyzerClient) {}

    refresh() {
        this.emitter.fire();
    }

    provideCodeLenses(document: vscode.TextDocument): vscode.CodeLens[] {
        const stats = this.client.getStatsByFile(document.uri.fsPath);
        if (stats.size === 0) return [];

        const lenses: vscode.CodeLens[] = [];

        for (const [line, s] of stats) {
            const vsLine = line - 1; // VS Code lines are 0-indexed
            if (vsLine < 0 || vsLine >= document.lineCount) continue;

            const range = new vscode.Range(vsLine, 0, vsLine, 0);
            const label = buildLabel(s.alloc_count, s.total_bytes, s.live_bytes);

            lenses.push(new vscode.CodeLens(range, {
                title: label,
                command: '',
                tooltip: `Function: ${s.function}\nTotal allocations: ${s.alloc_count}\nTotal bytes: ${formatBytes(s.total_bytes)}\nCurrently live: ${formatBytes(s.live_bytes)}`
            }));
        }

        return lenses;
    }
}

function buildLabel(count: number, total: number, live: number): string {
    const parts = [
        `⬆ ${count} alloc${count !== 1 ? 's' : ''}`,
        `${formatBytes(total)} total`,
    ];
    if (live > 0) parts.push(`${formatBytes(live)} live`);
    return parts.join('  ·  ');
}

function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}
