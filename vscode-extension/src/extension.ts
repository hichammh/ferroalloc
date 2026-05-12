import * as vscode from 'vscode';
import { AnalyzerClient } from './analyzerClient';
import { MemoryCodeLensProvider, formatBytes } from './codelens';
import { HeatmapDecorator } from './heatmap';
import { StatusBar } from './statusBar';

let poller: NodeJS.Timeout | undefined;
let client: AnalyzerClient;
let heatmap: HeatmapDecorator;
let statusBar: StatusBar;
let tracking = false;

export function activate(context: vscode.ExtensionContext): void {
    const config = vscode.workspace.getConfiguration('ferroalloc');
    const port: number = config.get('analyzerPort', 7778);
    const interval: number = config.get('refreshIntervalMs', 1000);

    client   = new AnalyzerClient(port);
    heatmap  = new HeatmapDecorator(client);
    statusBar = new StatusBar(client);
    const codelens = new MemoryCodeLensProvider(client);

    context.subscriptions.push(
        vscode.languages.registerCodeLensProvider({ language: 'rust' }, codelens),

        vscode.commands.registerCommand('ferroalloc.start', () => {
            startPolling(interval);
            vscode.window.showInformationMessage('Ferroalloc: memory tracking started');
        }),

        vscode.commands.registerCommand('ferroalloc.stop', () => {
            stopPolling();
            heatmap.clearAll();
            vscode.window.showInformationMessage('Ferroalloc: memory tracking stopped');
        }),

        vscode.commands.registerCommand('ferroalloc.toggle', () => {
            if (tracking) {
                vscode.commands.executeCommand('ferroalloc.stop');
            } else {
                vscode.commands.executeCommand('ferroalloc.start');
            }
        }),

        vscode.commands.registerCommand('ferroalloc.reset', async () => {
            await client.reset();
            heatmap.clearAll();
            codelens.refresh();
            vscode.window.showInformationMessage('Ferroalloc: data reset');
        }),

        vscode.commands.registerCommand('ferroalloc.showLeaks', async () => {
            const leaks = await client.fetchLeaks();
            if (leaks.length === 0) {
                vscode.window.showInformationMessage('Ferroalloc: no live leaks detected.');
                return;
            }
            const items = leaks.map(l => ({
                label: `$(warning) ${l.file}:${l.line}`,
                description: formatBytes(l.size),
                detail: `ptr: 0x${l.ptr.toString(16)}`,
            }));
            vscode.window.showQuickPick(items, {
                title: `${leaks.length} unfreed allocation(s)`,
                matchOnDescription: true,
            });
        }),

        // Auto-start when a debug session launches
        vscode.debug.onDidStartDebugSession(() => {
            if (!tracking) {
                startPolling(interval);
            }
        }),

        // Auto-stop when the debug session ends
        vscode.debug.onDidTerminateDebugSession(() => {
            stopPolling();
        }),

        { dispose: () => { stopPolling(); statusBar.dispose(); } },
    );
}

export function deactivate(): void {
    stopPolling();
}

function startPolling(intervalMs: number): void {
    stopPolling();
    tracking = true;
    poller = setInterval(() => client.refresh(), intervalMs);
}

function stopPolling(): void {
    tracking = false;
    if (poller) {
        clearInterval(poller);
        poller = undefined;
    }
}
