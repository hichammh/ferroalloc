import * as vscode from 'vscode';
import { AnalyzerClient } from './analyzerClient';

/**
 * Status bar item showing the connection state and total live bytes.
 * Click to toggle tracking on/off.
 */
export class StatusBar {
    private item: vscode.StatusBarItem;

    constructor(private client: AnalyzerClient) {
        this.item = vscode.window.createStatusBarItem(
            vscode.StatusBarAlignment.Right,
            100
        );
        this.item.command = 'ferroalloc.toggle';
        this.setIdle();
        this.item.show();

        client.on('connected',    () => this.setTracking(0));
        client.on('disconnected', () => this.setIdle());
        client.on('update', (stats: import('./analyzerClient').LineStats[]) => {
            const live = stats.reduce((sum, s) => sum + s.live_bytes, 0);
            this.setTracking(live);
        });
    }

    private setIdle(): void {
        this.item.text = '$(circle-slash) Ferroalloc';
        this.item.tooltip = 'Ferroalloc: analyzer not connected — click to start';
        this.item.color = new vscode.ThemeColor('statusBarItem.warningForeground');
    }

    private setTracking(liveBytes: number): void {
        const label = liveBytes > 0 ? formatBytes(liveBytes) + ' live' : 'tracking';
        this.item.text = `$(pulse) Ferroalloc: ${label}`;
        this.item.tooltip = 'Ferroalloc: tracking memory — click to stop';
        this.item.color = undefined;
    }

    dispose(): void {
        this.item.dispose();
    }
}

function formatBytes(bytes: number): string {
    if (bytes < 1024) { return `${bytes} B`; }
    if (bytes < 1024 * 1024) { return `${(bytes / 1024).toFixed(1)} KB`; }
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}
