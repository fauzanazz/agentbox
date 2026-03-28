import { describe, it } from "node:test";
import assert from "node:assert";

// Test type exports
import {
  AgentBoxClient,
  AgentBoxError,
  ExecSession,
  Sandbox,
  getToolDefinitions,
  handleToolCall,
} from "../dist/index.js";

// ── Exports ───────────────────────────────────────────────────

describe("Exports", () => {
  it("exports Sandbox", () => {
    assert.strictEqual(typeof Sandbox, "function");
  });

  it("exports AgentBoxClient", () => {
    assert.strictEqual(typeof AgentBoxClient, "function");
  });

  it("exports AgentBoxError", () => {
    assert.strictEqual(typeof AgentBoxError, "function");
  });

  it("exports ExecSession", () => {
    assert.strictEqual(typeof ExecSession, "function");
  });

  it("exports getToolDefinitions", () => {
    assert.strictEqual(typeof getToolDefinitions, "function");
  });

  it("exports handleToolCall", () => {
    assert.strictEqual(typeof handleToolCall, "function");
  });
});

// ── Sandbox ───────────────────────────────────────────────────

describe("Sandbox", () => {
  it("has static create method", () => {
    assert.strictEqual(typeof Sandbox.create, "function");
  });

  it("has static list method", () => {
    assert.strictEqual(typeof Sandbox.list, "function");
  });

  it("has static poolStatus method", () => {
    assert.strictEqual(typeof Sandbox.poolStatus, "function");
  });

  it("has static health method", () => {
    assert.strictEqual(typeof Sandbox.health, "function");
  });

  it("has all instance methods", () => {
    const methods = [
      "exec",
      "execStream",
      "execInteractive",
      "uploadContent",
      "download",
      "listFiles",
      "deleteFile",
      "mkdir",
      "sendSignal",
      "portForward",
      "listPortForwards",
      "removePortForward",
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

// ── AgentBoxClient ────────────────────────────────────────────

describe("AgentBoxClient", () => {
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

  // Auth tests

  it("has no apiKey by default", () => {
    const client = new AgentBoxClient();
    assert.strictEqual(client.apiKey, undefined);
  });

  it("accepts explicit apiKey", () => {
    const client = new AgentBoxClient(undefined, "my-secret");
    assert.strictEqual(client.apiKey, "my-secret");
  });

  it("appends token to WebSocket URL when apiKey is set", () => {
    const client = new AgentBoxClient("http://localhost:8080", "my-key");
    const wsUrl = client.wsUrl("/sandboxes/abc/ws");
    assert.ok(wsUrl.includes("?token=my-key"), `Expected token in WS URL: ${wsUrl}`);
  });

  it("does not append token to WebSocket URL when no apiKey", () => {
    const client = new AgentBoxClient("http://localhost:8080");
    const wsUrl = client.wsUrl("/sandboxes/abc/ws");
    assert.ok(!wsUrl.includes("token="), `Unexpected token in WS URL: ${wsUrl}`);
  });

  it("URL-encodes apiKey in WebSocket URL", () => {
    const client = new AgentBoxClient("http://localhost:8080", "key with spaces&special=chars");
    const wsUrl = client.wsUrl("/ws");
    assert.ok(wsUrl.includes("?token="), `Expected token param: ${wsUrl}`);
    assert.ok(!wsUrl.includes(" "), "Token should be URL-encoded");
  });

  it("has client-level API methods", () => {
    const methods = ["listSandboxes", "poolStatus", "health"];
    for (const m of methods) {
      assert.strictEqual(
        typeof AgentBoxClient.prototype[m],
        "function",
        `Missing method: ${m}`,
      );
    }
  });
});

// ── AgentBoxError ─────────────────────────────────────────────

describe("AgentBoxError", () => {
  it("extends Error", () => {
    const err = new AgentBoxError("test error", 404, '{"error":"not found"}');
    assert.ok(err instanceof Error);
    assert.ok(err instanceof AgentBoxError);
  });

  it("has statusCode and responseBody", () => {
    const err = new AgentBoxError("boom", 500, '{"error":"internal"}');
    assert.strictEqual(err.statusCode, 500);
    assert.strictEqual(err.responseBody, '{"error":"internal"}');
    assert.strictEqual(err.message, "boom");
    assert.strictEqual(err.name, "AgentBoxError");
  });
});

// ── ExecSession ───────────────────────────────────────────────

describe("ExecSession", () => {
  it("has expected methods", () => {
    const methods = ["events", "sendStdin", "sendSignal", "close"];
    for (const m of methods) {
      assert.strictEqual(
        typeof ExecSession.prototype[m],
        "function",
        `Missing method: ${m}`,
      );
    }
  });
});

// ── Tool Definitions ──────────────────────────────────────────

describe("Tool Definitions", () => {
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

// ── handleToolCall ────────────────────────────────────────────

describe("handleToolCall", () => {
  function mockSandbox(overrides = {}) {
    return {
      exec:
        overrides.exec ??
        (async (cmd) => ({ stdout: "output", stderr: "", exit_code: 0 })),
      uploadContent:
        overrides.uploadContent ?? (async (content, path) => {}),
      download:
        overrides.download ??
        (async (path) => new TextEncoder().encode("file content")),
    };
  }

  it("handles execute_code with Anthropic format", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "execute_code",
      input: { command: "echo hi" },
    });
    assert.strictEqual(result.stdout, "output");
    assert.strictEqual(result.exit_code, 0);
  });

  it("handles execute_code with OpenAI format", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      function: { name: "execute_code", arguments: { command: "ls" } },
    });
    assert.strictEqual(result.stdout, "output");
    assert.strictEqual(result.exit_code, 0);
  });

  it("handles execute_code with stringified arguments", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "execute_code",
      arguments: JSON.stringify({ command: "pwd" }),
    });
    assert.strictEqual(result.stdout, "output");
    assert.strictEqual(result.exit_code, 0);
  });

  it("returns error for execute_code missing command", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "execute_code",
      input: {},
    });
    assert.ok(result.error);
    assert.ok(result.error.toLowerCase().includes("command"));
  });

  it("handles write_file correctly", async () => {
    let calledContent, calledPath;
    const sandbox = mockSandbox({
      uploadContent: async (content, path) => {
        calledContent = content;
        calledPath = path;
      },
    });
    const result = await handleToolCall(sandbox, {
      name: "write_file",
      input: { path: "/tmp/test.txt", content: "hello world" },
    });
    assert.strictEqual(result.status, "written");
    assert.strictEqual(calledPath, "/tmp/test.txt");
    assert.deepStrictEqual(
      calledContent,
      new TextEncoder().encode("hello world"),
    );
  });

  it("returns error for write_file missing path", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "write_file",
      input: { content: "hello" },
    });
    assert.ok(result.error);
    assert.ok(result.error.toLowerCase().includes("path"));
  });

  it("returns error for write_file missing content", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "write_file",
      input: { path: "/tmp/test.txt" },
    });
    assert.ok(result.error);
    assert.ok(result.error.toLowerCase().includes("content"));
  });

  it("handles read_file correctly", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "read_file",
      input: { path: "/tmp/test.txt" },
    });
    assert.strictEqual(result.content, "file content");
  });

  it("returns error for read_file missing path", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "read_file",
      input: {},
    });
    assert.ok(result.error);
    assert.ok(result.error.toLowerCase().includes("path"));
  });

  it("returns error for unknown tool", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "delete_everything",
      input: {},
    });
    assert.ok(result.error);
    assert.ok(result.error.includes("Unknown"));
  });

  it("returns error for unparseable tool call", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {});
    assert.ok(result.error);
  });

  it("returns error for invalid JSON string arguments", async () => {
    const sandbox = mockSandbox();
    const result = await handleToolCall(sandbox, {
      name: "execute_code",
      arguments: "not json{",
    });
    assert.ok(result.error);
  });
});

// ── Tool Definition Schema Validation ─────────────────────────

describe("Tool Definition Schema Validation", () => {
  it("OpenAI format has correct nested structure", () => {
    const tools = getToolDefinitions("openai");
    for (const tool of tools) {
      assert.strictEqual(tool.type, "function");
      assert.ok(tool.function.name, "missing function.name");
      assert.ok(tool.function.description, "missing function.description");
      assert.strictEqual(tool.function.parameters.type, "object");
      assert.ok(tool.function.parameters.properties, "missing properties");
      assert.ok(
        Array.isArray(tool.function.parameters.required),
        "required should be an array",
      );
    }
  });

  it("Anthropic format has correct nested structure", () => {
    const tools = getToolDefinitions("anthropic");
    for (const tool of tools) {
      assert.ok(tool.name, "missing name");
      assert.ok(tool.description, "missing description");
      assert.strictEqual(tool.input_schema.type, "object");
    }
  });

  it("execute_code requires command", () => {
    const tools = getToolDefinitions("generic");
    const tool = tools.find((t) => t.name === "execute_code");
    assert.ok(tool, "execute_code tool not found");
    assert.deepStrictEqual(tool.required, ["command"]);
  });

  it("write_file requires path and content", () => {
    const tools = getToolDefinitions("generic");
    const tool = tools.find((t) => t.name === "write_file");
    assert.ok(tool, "write_file tool not found");
    assert.ok(tool.required.includes("path"), "missing path in required");
    assert.ok(
      tool.required.includes("content"),
      "missing content in required",
    );
  });

  it("read_file requires path", () => {
    const tools = getToolDefinitions("generic");
    const tool = tools.find((t) => t.name === "read_file");
    assert.ok(tool, "read_file tool not found");
    assert.deepStrictEqual(tool.required, ["path"]);
  });
});
