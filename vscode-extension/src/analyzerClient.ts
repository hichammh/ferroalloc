import * as http from 'http';

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

export class AnalyzerClient {
    private baseUrl: string;
    private cache: LineStats[] = [];

    constructor(port: number) {
        this.baseUrl = `http://127.0.0.1:${port}`;
    }

    async refresh(): Promise<void> {
        try {
            this.cache = await this.get<LineStats[]>('/snapshot');
        } catch {
            // Analyzer not running yet — keep stale data
        }
    }

    /** Returns the latest cached snapshot without triggering a network request. */
    getSnapshot(): LineStats[] {
        return this.cache;
    }

    /** Returns stats for a specific file, keyed by line number. */
    getStatsByFile(filePath: string): Map<number, LineStats> {
        const map = new Map<number, LineStats>();
        for (const stat of this.cache) {
            if (filePath.endsWith(stat.file) || stat.file.endsWith(filePath)) {
                map.set(stat.line, stat);
            }
        }
        return map;
    }

    async fetchLeaks(): Promise<LeakEntry[]> {
        return this.get<LeakEntry[]>('/leaks');
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
            http.get(`${this.baseUrl}${path}`, res => {
                let data = '';
                res.on('data', chunk => data += chunk);
                res.on('end', () => {
                    try { resolve(JSON.parse(data)); }
                    catch (e) { reject(e); }
                });
            }).on('error', reject);
        });
    }
}
