/** HTTP client for the AgentBox daemon API. Zero dependencies — uses native fetch. */
export declare class AgentBoxClient {
    readonly baseUrl: string;
    readonly apiKey: string | undefined;
    private readonly headers;
    constructor(url?: string, apiKey?: string);
    post(path: string, body: unknown): Promise<unknown>;
    postMultipart(path: string, form: FormData): Promise<unknown>;
    get(path: string, params?: Record<string, string>): Promise<unknown>;
    getBytes(path: string, params?: Record<string, string>): Promise<ArrayBuffer>;
    put(path: string, params?: Record<string, string>): Promise<unknown>;
    delete(path: string): Promise<unknown>;
    wsUrl(path: string): string;
    /** List all active sandboxes. */
    listSandboxes(): Promise<unknown[]>;
    /** Get pool status (warm VMs, capacity). */
    poolStatus(): Promise<unknown>;
    /** Health check (public, no auth required). */
    health(): Promise<unknown>;
}
