const endpoint = process.env.SETTLEBOT_EXPORT_URL;
const token = process.env.SETTLEBOT_EXPORT_TOKEN;

if (!endpoint || !token) {
  throw new Error("SETTLEBOT_EXPORT_URL and SETTLEBOT_EXPORT_TOKEN are required");
}

const baseUrl = new URL(endpoint);
if (baseUrl.protocol !== "https:") {
  throw new Error("SETTLEBOT_EXPORT_URL must use HTTPS");
}

let cursor;
for (;;) {
  const url = new URL(baseUrl);
  if (cursor) url.searchParams.set("cursor", cursor);
  const response = await fetch(url, {
    headers: { authorization: `Bearer ${token}` },
    redirect: "error",
  });
  if (!response.ok) {
    throw new Error(`Export failed with HTTP ${response.status}`);
  }
  const chunk = await response.json();
  process.stdout.write(`${JSON.stringify(chunk)}\n`);
  cursor = chunk.next_cursor;
  if (!cursor) break;
}
