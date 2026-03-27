/** HTTP client for the AgentBox daemon API. Zero dependencies — uses native fetch. */
export declare class AgentBoxClient {
    readonly baseUrl: string;
    constructor(url?: string);
    post(path: string, body: unknown): Promise<unknown>;
    postMultipart(path: string, form: FormData): Promise<unknown>;
    get(path: string, params?: Record<string, string>): Promise<unknown>;
    getBytes(path: string, params?: Record<string, string>): Promise<ArrayBuffer>;
    delete(path: string): Promise<unknown>;
    wsUrl(path: string): string;
}
