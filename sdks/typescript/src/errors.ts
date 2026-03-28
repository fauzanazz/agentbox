/** Custom error class for AgentBox API errors. */
export class AgentBoxError extends Error {
  readonly statusCode: number;
  readonly responseBody: string;

  constructor(message: string, statusCode: number, responseBody: string) {
    super(message);
    this.name = "AgentBoxError";
    this.statusCode = statusCode;
    this.responseBody = responseBody;
  }
}

/** Parse response body and throw a typed AgentBoxError. */
export async function throwForStatus(
  method: string,
  path: string,
  resp: Response,
): Promise<never> {
  const body = await resp.text().catch(() => "");

  // Try to extract error message from JSON
  let message = body;
  try {
    const data = JSON.parse(body);
    if (data && typeof data.error === "string") {
      message = data.error;
    }
  } catch {
    // Not JSON, use raw body
  }

  throw new AgentBoxError(
    `${method} ${path} failed (${resp.status}): ${message}`,
    resp.status,
    body,
  );
}
