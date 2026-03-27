/** HTTP client for the AgentBox daemon API. Zero dependencies — uses native fetch. */
export class AgentBoxClient {
  readonly baseUrl: string;

  constructor(url?: string) {
    this.baseUrl = (
      url ?? process.env.AGENTBOX_URL ?? "http://localhost:8080"
    ).replace(/\/+$/, "");
  }

  async post(path: string, body: unknown): Promise<unknown> {
    const resp = await fetch(`${this.baseUrl}${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!resp.ok) {
      const text = await resp.text().catch(() => "");
      throw new Error(`POST ${path} failed (${resp.status}): ${text}`);
    }
    return resp.json();
  }

  async postMultipart(path: string, form: FormData): Promise<unknown> {
    const resp = await fetch(`${this.baseUrl}${path}`, {
      method: "POST",
      body: form,
    });
    if (!resp.ok) {
      const text = await resp.text().catch(() => "");
      throw new Error(`POST ${path} failed (${resp.status}): ${text}`);
    }
    return resp.json();
  }

  async get(path: string, params?: Record<string, string>): Promise<unknown> {
    const url = new URL(`${this.baseUrl}${path}`);
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        url.searchParams.set(k, v);
      }
    }
    const resp = await fetch(url.toString());
    if (!resp.ok) {
      const text = await resp.text().catch(() => "");
      throw new Error(`GET ${path} failed (${resp.status}): ${text}`);
    }
    return resp.json();
  }

  async getBytes(
    path: string,
    params?: Record<string, string>,
  ): Promise<ArrayBuffer> {
    const url = new URL(`${this.baseUrl}${path}`);
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        url.searchParams.set(k, v);
      }
    }
    const resp = await fetch(url.toString());
    if (!resp.ok) {
      const text = await resp.text().catch(() => "");
      throw new Error(`GET ${path} failed (${resp.status}): ${text}`);
    }
    return resp.arrayBuffer();
  }

  async delete(path: string): Promise<unknown> {
    const resp = await fetch(`${this.baseUrl}${path}`, { method: "DELETE" });
    if (!resp.ok) {
      const text = await resp.text().catch(() => "");
      throw new Error(`DELETE ${path} failed (${resp.status}): ${text}`);
    }
    return resp.json();
  }

  wsUrl(path: string): string {
    return (
      this.baseUrl
        .replace("http://", "ws://")
        .replace("https://", "wss://") + path
    );
  }
}
