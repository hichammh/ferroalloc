import * as vscode from 'vscode';
import { AnalyzerClient } from './analyzerClient';

// Decoration types from cold (green) to hot (red) — 5 intensity levels
const LEVELS = [
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(0,255,0,0.08)' }),
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(128,255,0,0.12)' }),
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(255,200,0,0.15)' }),
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(255,100,0,0.18)' }),
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(255,0,0,0.22)', border: '1px solid rgba(255,0,0,0.4)' }),
];

const LEAK_DECORATION = vscode.window.createTextEditorDecorationType({
    backgroundColor: 'rgba(255,0,0,0.15)',
    border: '1px solid rgba(220,0,0,0.6)',
    after: { contentText: '  ⚠ potential leak', color: 'rgba(220,80,80,0.9)', fontStyle: 'italic' }
});

export class HeatmapDecorator {
    constructor(private client: AnalyzerClient) {}

    refresh() {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== 'rust') return;

        const stats = this.client.getStatsByFile(editor.document.uri.fsPath);
        if (stats.size === 0) return;

        // Determine max allocation to normalize intensity
        let maxBytes = 0;
        for (const s of stats.values()) {
            if (s.total_bytes > maxBytes) maxBytes = s.total_bytes;
        }

        const buckets: vscode.Range[][] = LEVELS.map(() => []);
        const leakRanges: vscode.Range[] = [];

        for (const [line, s] of stats) {
            const vsLine = line - 1;
            if (vsLine < 0 || vsLine >= editor.document.lineCount) continue;

            const range = editor.document.lineAt(vsLine).range;
            const intensity = maxBytes > 0 ? s.total_bytes / maxBytes : 0;
            const level = Math.min(LEVELS.length - 1, Math.floor(intensity * LEVELS.length));
            buckets[level].push(range);

            if (s.live_bytes > 0) {
                leakRanges.push(range);
            }
        }

        LEVELS.forEach((dec, i) => editor.setDecorations(dec, buckets[i]));
        editor.setDecorations(LEAK_DECORATION, leakRanges);
    }
}
