import * as vscode from 'vscode';
import { AnalyzerClient } from './analyzerClient';

// Five intensity levels: cold (green) → hot (red)
const LEVELS = [
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(0,200,0,0.08)' }),
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(150,220,0,0.11)' }),
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(255,200,0,0.14)' }),
    vscode.window.createTextEditorDecorationType({ backgroundColor: 'rgba(255,100,0,0.17)' }),
    vscode.window.createTextEditorDecorationType({
        backgroundColor: 'rgba(255,0,0,0.20)',
        border: '1px solid rgba(255,0,0,0.35)',
    }),
];

// Decoration for lines with unfreed allocations (potential leaks)
const LEAK_DECORATION = vscode.window.createTextEditorDecorationType({
    backgroundColor: 'rgba(255,0,0,0.12)',
    border: '1px solid rgba(200,0,0,0.50)',
    after: {
        contentText: '  ⚠ potential leak',
        color: 'rgba(220,80,80,0.85)',
        fontStyle: 'italic',
    },
});

export class HeatmapDecorator {
    constructor(private client: AnalyzerClient) {
        // Refresh heatmap on every data update
        client.on('update', () => this.refresh());

        // Clear decorations when switching to a non-Rust file
        vscode.window.onDidChangeActiveTextEditor(editor => {
            if (!editor || editor.document.languageId !== 'rust') {
                this.clearAll();
            } else {
                this.refresh();
            }
        });
    }

    refresh(): void {
        const config = vscode.workspace.getConfiguration('ferroalloc');
        if (!config.get<boolean>('heatmapEnabled', true)) {
            this.clearAll();
            return;
        }

        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== 'rust') {
            return;
        }

        const stats = this.client.getStatsByFile(editor.document.uri.fsPath);
        if (stats.size === 0) {
            this.clearAll();
            return;
        }

        // Determine max allocation to normalise intensity
        let maxBytes = 0;
        for (const s of stats.values()) {
            if (s.total_bytes > maxBytes) { maxBytes = s.total_bytes; }
        }

        const buckets: vscode.Range[][] = LEVELS.map(() => []);
        const leakRanges: vscode.Range[] = [];

        for (const [line, s] of stats) {
            const vsLine = line - 1;
            if (vsLine < 0 || vsLine >= editor.document.lineCount) { continue; }

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

    clearAll(): void {
        const editor = vscode.window.activeTextEditor;
        if (!editor) { return; }
        LEVELS.forEach(dec => editor.setDecorations(dec, []));
        editor.setDecorations(LEAK_DECORATION, []);
    }
}
