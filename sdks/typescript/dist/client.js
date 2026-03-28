import { throwForStatus } from "./errors.js";
/** HTTP client for the AgentBox daemon API. Zero dependencies — uses native fetch. */
export class AgentBoxClient {
    baseUrl;
    apiKey;
    headers;
    constructor(url, apiKey) {
        this.baseUrl = (url ?? process.env.AGENTBOX_URL ?? "http://localhost:8080").replace(/\/+$/, "");
        this.apiKey = apiKey ?? process.env.AGENTBOX_API_KEY ?? undefined;
        this.headers = {};
        if (this.apiKey) {
            this.headers["Authorization"] = `Bearer ${this.apiKey}`;
        }
    }
    async post(path, body) {
        const resp = await fetch(`${this.baseUrl}${path}`, {
            method: "POST",
            headers: { ...this.headers, "Content-Type": "application/json" },
            body: JSON.stringify(body),
        });
        if (!resp.ok)
            await throwForStatus("POST", path, resp);
        return resp.json();
    }
    async postMultipart(path, form) {
        const resp = await fetch(`${this.baseUrl}${path}`, {
            method: "POST",
            headers: this.headers,
            body: form,
        });
        if (!resp.ok)
            await throwForStatus("POST", path, resp);
        return resp.json();
    }
    async get(path, params) {
        const url = new URL(`${this.baseUrl}${path}`);
        if (params) {
            for (const [k, v] of Object.entries(params)) {
                url.searchParams.set(k, v);
            }
        }
        const resp = await fetch(url.toString(), { headers: this.headers });
        if (!resp.ok)
            await throwForStatus("GET", path, resp);
        return resp.json();
    }
    async getBytes(path, params) {
        const url = new URL(`${this.baseUrl}${path}`);
        if (params) {
            for (const [k, v] of Object.entries(params)) {
                url.searchParams.set(k, v);
            }
        }
        const resp = await fetch(url.toString(), { headers: this.headers });
        if (!resp.ok)
            await throwForStatus("GET", path, resp);
        return resp.arrayBuffer();
    }
    async put(path, params) {
        const url = new URL(`${this.baseUrl}${path}`);
        if (params) {
            for (const [k, v] of Object.entries(params)) {
                url.searchParams.set(k, v);
            }
        }
        const resp = await fetch(url.toString(), {
            method: "PUT",
            headers: this.headers,
        });
        if (!resp.ok)
            await throwForStatus("PUT", path, resp);
        return resp.json();
    }
    async delete(path) {
        const resp = await fetch(`${this.baseUrl}${path}`, {
            method: "DELETE",
            headers: this.headers,
        });
        if (!resp.ok)
            await throwForStatus("DELETE", path, resp);
        return resp.json();
    }
    wsUrl(path) {
        const base = this.baseUrl
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        if (this.apiKey) {
            return `${base}${path}?token=${encodeURIComponent(this.apiKey)}`;
        }
        return `${base}${path}`;
    }
    // ── Client-level API methods ────────────────────────────────
    /** List all active sandboxes. */
    async listSandboxes() {
        return (await this.get("/sandboxes"));
    }
    /** Get pool status (warm VMs, capacity). */
    async poolStatus() {
        return this.get("/pool/status");
    }
    /** Health check (public, no auth required). */
    async health() {
        return this.get("/health");
    }
}
