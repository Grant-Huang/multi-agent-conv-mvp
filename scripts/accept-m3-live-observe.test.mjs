import assert from "node:assert/strict";
import test from "node:test";
import { checkLiveObserve } from "./accept-m3-live-observe.mjs";

const states = { odoo: "confirmed", openmes: "PENDING", carbon: "Ready" };

function fixture(change = (body) => body) {
  const calls = [];
  return {
    origin: "http://127.0.0.1:8787", token: "test-token",
    worlds: ["odoo", "openmes", "carbon"].map((vendor) => ({
      vendor, world: `world-${vendor}`, visible: "own-work-order", hidden: "hidden-work-order",
      source: `site-${vendor}`, sourceStatus: states[vendor],
    })),
    calls,
    async request(url, init) {
      const body = JSON.parse(init.body);
      assert.equal(url.pathname, "/api/world/prod/observe");
      assert.equal(init.redirect, "error");
      assert.equal(body.selection.type, "work_order");
      calls.push(body.world_id);
      const envelope = { protocol_version: 1, request_id: body.request_id };
      if (!init.headers.Authorization) return Response.json({ ...envelope, code: "AUTHENTICATION_REQUIRED" }, { status: 401 });
      assert.equal(init.headers.Authorization, "Bearer test-token");
      if (body.selection.id === "hidden-work-order") return Response.json({ ...envelope, code: "ORDER_NOT_VISIBLE" }, { status: 404 });
      const asOf = "2026-11-23T00:00:00Z";
      return Response.json(change({
        ...envelope, world_id: body.world_id, provenance: "production", consistency: "per_source",
        cursor: null, publish_seq: null, events: [], as_of: asOf,
        objects: [{ ref: body.selection, source: body.world_id.replace("world-", "site-"), observed_at: asOf,
          state: { display_number: "WO-42", source_status: states[body.world_id.replace("world-", "")], normalized_status: null, mapping_version: "fixture" } }],
      }));
    },
  };
}

test("runs the same authorized lookup and rejection sequence for three sources", async () => {
  const config = fixture();
  assert.deepEqual(await checkLiveObserve(config), {
    vendors: ["odoo", "openmes", "carbon"], anonymous: 401, visible: 200, hidden: 404,
  });
  assert.equal(config.calls.length, 9);
});

test("rejects a work-order response with a synthetic publish cursor", async () => {
  await assert.rejects(checkLiveObserve(fixture((body) => ({ ...body, cursor: "stub-cursor" }))), /projection/);
});

test("rejects another source's observation", async () => {
  await assert.rejects(checkLiveObserve(fixture((body) => {
    body.objects[0].source = "site-other";
    return body;
  })), /projection/);
});

test("rejects an unsupported origin before using a credential", async () => {
  const config = fixture();
  await assert.rejects(checkLiveObserve({ ...config, origin: "http://example.test" }), /Invalid Observe origin/);
  assert.equal(config.calls.length, 0);
});
