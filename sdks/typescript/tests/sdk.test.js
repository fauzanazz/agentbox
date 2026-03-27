import { describe, it } from "node:test";
import assert from "node:assert";

// Test type exports
import { Sandbox } from "../dist/index.js";

describe("Sandbox", () => {
  it("has static create method", () => {
    assert.strictEqual(typeof Sandbox.create, "function");
  });

  it("has instance methods", () => {
    // Verify prototype has all expected methods
    const methods = [
      "exec",
      "execStream",
      "uploadContent",
      "download",
      "listFiles",
      "info",
      "destroy",
      "toolDefinitions",
      "handleToolCall",
    ];
    for (const m of methods) {
      assert.strictEqual(
        typeof Sandbox.prototype[m],
        "function",
        `Missing method: ${m}`,
      );
    }
  });
});

describe("AgentBoxClient", async () => {
  const { AgentBoxClient } = await import("../dist/client.js");

  it("uses default URL", () => {
    const client = new AgentBoxClient();
    assert.strictEqual(client.baseUrl, "http://localhost:8080");
  });

  it("accepts custom URL", () => {
    const client = new AgentBoxClient("http://custom:9090");
    assert.strictEqual(client.baseUrl, "http://custom:9090");
  });

  it("strips trailing slashes", () => {
    const client = new AgentBoxClient("http://custom:9090///");
    assert.strictEqual(client.baseUrl, "http://custom:9090");
  });

  it("generates WebSocket URL", () => {
    const client = new AgentBoxClient("http://localhost:8080");
    assert.strictEqual(
      client.wsUrl("/sandboxes/abc/ws"),
      "ws://localhost:8080/sandboxes/abc/ws",
    );
  });

  it("handles HTTPS to WSS conversion", () => {
    const client = new AgentBoxClient("https://api.example.com");
    assert.strictEqual(
      client.wsUrl("/ws"),
      "wss://api.example.com/ws",
    );
  });
});

describe("Tool Definitions", async () => {
  const { getToolDefinitions } = await import("../dist/tools.js");

  it("returns OpenAI format by default", () => {
    const tools = getToolDefinitions();
    assert.strictEqual(tools.length, 3);
    assert.strictEqual(tools[0].type, "function");
    assert.strictEqual(tools[0].function.name, "execute_code");
  });

  it("returns Anthropic format", () => {
    const tools = getToolDefinitions("anthropic");
    assert.strictEqual(tools.length, 3);
    assert.ok(tools[0].input_schema);
    assert.strictEqual(tools[0].name, "execute_code");
  });

  it("returns generic format", () => {
    const tools = getToolDefinitions("generic");
    assert.strictEqual(tools.length, 3);
    assert.strictEqual(tools[0].name, "execute_code");
    assert.ok(tools[0].parameters);
  });

  it("has all three tools", () => {
    const tools = getToolDefinitions("generic");
    const names = tools.map((t) => t.name);
    assert.deepStrictEqual(names, [
      "execute_code",
      "write_file",
      "read_file",
    ]);
  });
});
