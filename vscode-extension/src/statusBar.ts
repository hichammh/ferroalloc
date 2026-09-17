import * as vscode from 'vscode';
import { AnalyzerClient, Health, LineStats } from './analyzerClient';
import { formatBytes } from './format';

// Advice shown when the analyzer receives events it cannot attribute to a line.
// This is by far the most common setup failure: without debug symbols the probe
// resolves no file, every event is discarded, and the views stay empty.
const NO_SYMBOLS_HINT = [
    'Ferroalloc: events received but no source line resolved.',
    '',
    'Your program was most likely built without debug symbols.',
    'Add this to the Cargo.toml of the program under test:',
    '',
    '    [profile.release]',
    '    debug = true',
    '',
    'then rebuild and run again.',
].join('\n');

/**
 * Status bar item showing the connection state and total live bytes.
 * Click to toggle tracking on/off.
 */
export class StatusBar {
    private item: vscode.StatusBarItem;
    private health: Health | undefined;

    constructor(private client: AnalyzerClient) {
        this.item = vscode.window.createStatusBarItem(
            vscode.StatusBarAlignment.Right,
            100
        );
        this.item.command = 'ferroalloc.toggle';
        this.setIdle();
        this.item.show();

        client.on('connected',    () => this.setTracking(0));
        client.on('disconnected', () => { this.health = undefined; this.setIdle(); });
        client.on('health', (health: Health) => { this.health = health; });
        client.on('update', (stats: LineStats[]) => {
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
        const h = this.health;

        // Receiving events but resolving none is a setup problem, not a result:
        // say so instead of displaying a reassuring "tracking" over an empty view.
        if (h && h.events_received > 0 && h.events_resolved === 0) {
            this.item.text = '$(warning) Ferroalloc: no debug symbols';
            this.item.tooltip = NO_SYMBOLS_HINT;
            this.item.color = new vscode.ThemeColor('statusBarItem.warningForeground');
            return;
        }

        const label = liveBytes > 0 ? formatBytes(liveBytes) + ' live' : 'tracking';
        const dropped = h?.events_dropped ?? 0;

        // Dropped events mean allocations arrived without their matching frees,
        // so the live byte counts below are an over-estimate. Better to say it.
        if (dropped > 0) {
            this.item.text = `$(warning) Ferroalloc: ${label}`;
            this.item.tooltip = [
                `Ferroalloc: tracking memory — click to stop`,
                '',
                `${dropped} event(s) dropped: the probe produced them faster than`,
                'the analyzer could drain them. Live byte counts are an',
                'over-estimate. Lower the allocation volume or call',
                'ferroalloc_probe::set_sample_rate(n) to record 1 in n blocks.',
            ].join('\n');
            this.item.color = new vscode.ThemeColor('statusBarItem.warningForeground');
            return;
        }

        this.item.text = `$(pulse) Ferroalloc: ${label}`;
        this.item.tooltip = 'Ferroalloc: tracking memory — click to stop';
        this.item.color = undefined;
    }

    dispose(): void {
        this.item.dispose();
    }
}
