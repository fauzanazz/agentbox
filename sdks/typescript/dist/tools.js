const SANDBOX_TOOLS = [
    {
        name: "execute_code",
        description: "Execute a bash command or script in the sandbox. Use this to run code, install packages, or perform any shell operation.",
        parameters: {
            command: { type: "string", description: "The bash command to execute" },
        },
        required: ["command"],
    },
    {
        name: "write_file",
        description: "Write content to a file in the sandbox.",
        parameters: {
            path: { type: "string", description: "Absolute path in the sandbox" },
            content: { type: "string", description: "File content to write" },
        },
        required: ["path", "content"],
    },
    {
        name: "read_file",
        description: "Read the contents of a file in the sandbox.",
        parameters: {
            path: { type: "string", description: "Absolute path in the sandbox" },
        },
        required: ["path"],
    },
];
/** Return tool definitions in the specified format. */
export function getToolDefinitions(format = "openai") {
    if (format === "openai") {
        return SANDBOX_TOOLS.map((t) => ({
            type: "function",
            function: {
                name: t.name,
                description: t.description,
                parameters: {
                    type: "object",
                    properties: t.parameters,
                    required: t.required,
                },
            },
        }));
    }
    if (format === "anthropic") {
        return SANDBOX_TOOLS.map((t) => ({
            name: t.name,
            description: t.description,
            input_schema: {
                type: "object",
                properties: t.parameters,
                required: t.required,
            },
        }));
    }
    return SANDBOX_TOOLS;
}
/** Execute an LLM tool call against a sandbox. Supports OpenAI and Anthropic formats. */
export async function handleToolCall(sandbox, toolCall) {
    const name = toolCall.name ??
        toolCall.function?.name;
    let args = toolCall.input ??
        toolCall.arguments ??
        toolCall.function?.arguments;
    if (typeof args === "string") {
        try {
            args = JSON.parse(args);
        }
        catch {
            return { error: "Invalid JSON in tool call arguments" };
        }
    }
    if (!name || typeof args !== "object" || args === null) {
        return { error: "Could not parse tool call" };
    }
    if (name === "execute_code") {
        if (!args.command)
            return { error: "Missing required parameter: command" };
        const result = await sandbox.exec(args.command);
        return {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
        };
    }
    if (name === "write_file") {
        const missing = ["path", "content"].filter((k) => !(k in args));
        if (missing.length)
            return { error: `Missing required parameters: ${missing.join(", ")}` };
        const content = args.content;
        if (typeof content !== "string")
            return { error: "Parameter 'content' must be a string" };
        await sandbox.uploadContent(new TextEncoder().encode(content), args.path);
        return { status: "written", path: args.path };
    }
    if (name === "read_file") {
        if (!args.path)
            return { error: "Missing required parameter: path" };
        const data = await sandbox.download(args.path);
        return { content: new TextDecoder().decode(data) };
    }
    return { error: `Unknown tool: ${name}` };
}
