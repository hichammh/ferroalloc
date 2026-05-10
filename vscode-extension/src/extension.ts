import * as vscode from 'vscode';
import { MemoryCodeLensProvider } from './codelens';
import { HeatmapDecorator } from './heatmap';
import { AnalyzerClient } from './analyzerClient';

let poller: NodeJS.Timeout | undefined;
let client: AnalyzerClient;
let codelens: MemoryCodeLensProvider;
let heatmap: HeatmapDecorator;

export function activate(context: vscode.ExtensionContext) {
    const config = vscode.workspace.getConfiguration('ferroalloc');
    const port: number = config.get('analyzerPort', 7778);
    const interval: number = config.get('refreshIntervalMs', 1000);

    client   = new AnalyzerClient(port);
    codelens = new MemoryCodeLensProvider(client);
    heatmap  = new HeatmapDecorator(client);

    context.subscriptions.push(
        vscode.languages.registerCodeLensProvider({ language: 'rust' }, codelens),

        vscode.commands.registerCommand('ferroalloc.start', () => {
            startPolling(interval);
            vscode.window.showInformationMessage('Ferroalloc: memory tracking started');
        }),

        vscode.commands.registerCommand('ferroalloc.stop', () => {
            stopPolling();
            vscode.window.showInformationMessage('Ferroalloc: memory tracking stopped');
        }),

        vscode.commands.registerCommand('ferroalloc.showLeaks', async () => {
            const leaks = await client.fetchLeaks();
            if (leaks.length === 0) {
                vscode.window.showInformationMessage('No live leaks detected.');
                return;
            }
            const items = leaks.map(l =>
                `${l.file}:${l.line}  —  ${formatBytes(l.size)}`
            );
            vscode.window.showQuickPick(items, { title: `${leaks.length} live allocation(s) not freed` });
        })
    );
}

export function deactivate() {
    stopPolling();
}

function startPolling(intervalMs: number) {
    stopPolling();
    poller = setInterval(async () => {
        await client.refresh();
        codelens.refresh();
        heatmap.refresh();
    }, intervalMs);
}

function stopPolling() {
    if (poller) {
        clearInterval(poller);
        poller = undefined;
    }
}

function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}
