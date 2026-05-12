import * as http from 'http';
import { EventEmitter } from 'events';

export interface LineStats {
    file: string;
    line: number;
    function: string;
    alloc_count: number;
    total_bytes: number;
    live_bytes: number;
}

export interface LeakEntry {
    ptr: number;
    file: string;
    line: number;
    size: number;
}

export interface LeakGroup {
    function: string;
    file: string;
    leak_count: number;
    leaked_bytes: number;
    entries: LeakEntry[];
}

export interface LeakReport {
    total_leaked_bytes: number;
    total_leak_count: number;
    groups: LeakGroup[];
}

export interface DiffEntry {
    file: string;
    line: number;
    function: string;
    delta_alloc_count: number;
    delta_total_bytes: number;
    delta_live_bytes: number;
}

export interface SnapshotDiff {
    increased: DiffEntry[];
    decreased: DiffEntry[];
    new_lines: DiffEntry[];
    total_delta_bytes: number;
}

/**
 * HTTP client for the ferroalloc-analyzer API.
 * Emits 'update' whenever the snapshot is refreshed successfully.
 * Emits 'connected' / 'disconnected' on state changes.
 */
export class AnalyzerClient extends EventEmitter {
    private baseUrl: string;
    private port: number;
    private cache: LineStats[] = [];
    private _connected = false;

    constructor(port: number) {
        super();
        this.port = port;
        this.baseUrl = `http://127.0.0.1:${port}`;
    }

    get connected(): boolean {
        return this._connected;
    }

    /** Fetch latest stats from the analyzer and update the cache. */
    async refresh(): Promise<void> {
        try {
            this.cache = await this.get<LineStats[]>('/snapshot');
            if (!this._connected) {
                this._connected = true;
                this.emit('connected');
            }
            this.emit('update', this.cache);
        } catch {
            if (this._connected) {
                this._connected = false;
                this.emit('disconnected');
            }
        }
    }

    /** Returns the latest cached snapshot without a network request. */
    getSnapshot(): LineStats[] {
        return this.cache;
    }

    /**
     * Returns stats for a specific file, keyed by line number.
     * Matches by suffix so absolute vs relative paths both work.
     */
    getStatsByFile(filePath: string): Map<number, LineStats> {
        const map = new Map<number, LineStats>();
        const normalized = filePath.replace(/\\/g, '/');
        for (const stat of this.cache) {
            const statFile = stat.file.replace(/\\/g, '/');
            if (normalized.endsWith(statFile) || statFile.endsWith(normalized)) {
                map.set(stat.line, stat);
            }
        }
        return map;
    }

    async fetchLeaks(): Promise<LeakEntry[]> {
        return this.get<LeakEntry[]>('/leaks');
    }

    async fetchLeakReport(minBytes = 0): Promise<LeakReport> {
        return this.get<LeakReport>(`/leak-report?min_bytes=${minBytes}`);
    }

    /** Save the current snapshot as baseline for diff comparison. */
    async saveBaseline(): Promise<void> {
        await this.post('/baseline');
    }

    /** Get diff between saved baseline and current snapshot. */
    async fetchDiff(): Promise<SnapshotDiff> {
        return this.get<SnapshotDiff>('/diff');
    }

    async reset(): Promise<void> {
        await this.post('/reset');
        this.cache = [];
        this.emit('update', this.cache);
    }

    async isHealthy(): Promise<boolean> {
        try {
            await this.get('/health');
            return true;
        } catch {
            return false;
        }
    }

    private get<T>(path: string): Promise<T> {
        return new Promise((resolve, reject) => {
            const req = http.get(`${this.baseUrl}${path}`, { timeout: 2000 }, res => {
                let data = '';
                res.on('data', chunk => (data += chunk));
                res.on('end', () => {
                    try { resolve(JSON.parse(data)); }
                    catch (e) { reject(e); }
                });
            });
            req.on('error', reject);
            req.on('timeout', () => { req.destroy(); reject(new Error('timeout')); });
        });
    }

    private post(path: string): Promise<void> {
        return new Promise((resolve, reject) => {
            const req = http.request(
                {
                    hostname: '127.0.0.1',
                    port: this.port,
                    path,
                    method: 'POST',
                    timeout: 2000,
                },
                res => { res.resume(); res.on('end', resolve); }
            );
            req.on('error', reject);
            req.on('timeout', () => { req.destroy(); reject(new Error('timeout')); });
            req.end();
        });
    }
}
