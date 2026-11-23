// Candidate acceptance for a pre-authorized, read-only M3 integration deployment.
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

class AcceptanceError extends Error {}
const endpointPath = "/api/world/prod/observe";
const vendors = ["carbon", "odoo", "openmes"];
const present = (value) => typeof value === "string" && value.trim().length > 0;

export async function checkLiveObserve({ origin, token, worlds, request = fetch }) {
  if (!URL.canParse(origin)) throw new AcceptanceError("Invalid Observe origin");
  const url = new URL(origin);
  const loopback = ["127.0.0.1", "[::1]"].includes(url.hostname);
  if (!(url.protocol === "https:" || url.protocol === "http:" && loopback)
    || url.username || url.password || url.search || url.hash || url.pathname !== "/") {
    throw new AcceptanceError("Invalid Observe origin");
  }
  if (!present(token) || !Array.isArray(worlds)
    || worlds.length !== vendors.length
    || new Set(worlds.map((world) => world?.vendor)).size !== vendors.length
    || worlds.some((world) => !vendors.includes(world?.vendor))
    || worlds.some((world) => ![world.world, world.visible, world.hidden, world.source, world.sourceStatus].every(present)
      || world.visible === world.hidden)
    || new Set(worlds.map((world) => world.world)).size !== vendors.length
    || new Set(worlds.map((world) => world.source)).size !== vendors.length) {
    throw new AcceptanceError("Provide three distinct world and source bindings with two references each");
  }

  async function read(world, ref, authenticated, expectedStatus, expectedCode) {
    const requestId = `m3-live-${world.vendor}-${expectedStatus}`;
    const response = await request(new URL(endpointPath, url), {
      method: "POST",
      redirect: "error",
      headers: { "Content-Type": "application/json", ...(authenticated ? { Authorization: `Bearer ${token}` } : {}) },
      body: JSON.stringify({ world_id: world.world, request_id: requestId, selection: { type: "work_order", id: ref } }),
      signal: AbortSignal.timeout(20_000),
    });
    if (response.status !== expectedStatus) {
      throw new AcceptanceError(`${world.vendor}: expected HTTP ${expectedStatus}, got ${response.status}`);
    }
    const body = await response.json();
    if (body?.protocol_version !== 1 || body.request_id !== requestId) {
      throw new AcceptanceError(`${world.vendor}: incompatible response envelope`);
    }
    if (expectedCode) {
      if (body.code !== expectedCode || body.objects !== undefined) {
        throw new AcceptanceError(`${world.vendor}: incompatible error envelope`);
      }
      return;
    }
    const item = body.objects?.[0];
    if (body.world_id !== world.world || body.provenance !== "production"
      || body.consistency !== "per_source" || body.cursor !== null || body.publish_seq !== null
      || !Array.isArray(body.events) || body.events.length !== 0
      || !Array.isArray(body.objects) || body.objects.length !== 1
      || item?.ref?.type !== "work_order" || item.ref.id !== ref || item.source !== world.source
      || !present(body.as_of) || !Number.isFinite(Date.parse(body.as_of)) || item.observed_at !== body.as_of
      || !present(item.state?.display_number) || item.state.source_status !== world.sourceStatus
      || item.state.normalized_status !== null || !present(item.state.mapping_version)) {
      throw new AcceptanceError(`${world.vendor}: incompatible work-order projection`);
    }
  }

  for (const world of worlds) {
    await read(world, world.visible, false, 401, "AUTHENTICATION_REQUIRED");
    await read(world, world.visible, true, 200);
    await read(world, world.hidden, true, 404, "ORDER_NOT_VISIBLE");
  }
  return { vendors: worlds.map((world) => world.vendor), anonymous: 401, visible: 200, hidden: 404 };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const worlds = JSON.parse(await readFile(process.argv[2], "utf8"));
    console.log(JSON.stringify(await checkLiveObserve({
      origin: process.env.AN_M3_OBSERVE_ORIGIN,
      token: process.env.AN_M3_OBSERVE_TOKEN,
      worlds,
    })));
  } catch (error) {
    console.error(error instanceof AcceptanceError ? error.message : "M3 Observe acceptance failed without safe diagnostics");
    process.exitCode = 1;
  }
}
