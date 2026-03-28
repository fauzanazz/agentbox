import type { Sandbox } from "./sandbox.js";
import type { ToolResult } from "./types.js";
/** Return tool definitions in the specified format. */
export declare function getToolDefinitions(format?: "openai" | "anthropic" | "generic"): unknown[];
/** Execute an LLM tool call against a sandbox. Supports OpenAI and Anthropic formats. */
export declare function handleToolCall(sandbox: Sandbox, toolCall: Record<string, unknown>): Promise<ToolResult>;
