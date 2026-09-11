import assert from "node:assert/strict";
import test from "node:test";

import { composeCommand } from "./ackplane-compose.mjs";

test("composeCommand honors an explicit binary", () => {
  assert.equal(
    composeCommand({ MINDLEAK_COMPOSE_BIN: "podman" }, () =>
      assert.fail("an explicit binary should not be probed"),
    ),
    "podman",
  );
});

test("composeCommand selects the first available implementation", () => {
  const probes = [];
  const command = composeCommand({}, (candidate) => {
    probes.push(candidate);
    if (candidate === "docker") {
      throw new Error("docker unavailable");
    }
  });

  assert.equal(command, "podman");
  assert.deepEqual(probes, ["docker", "podman"]);
});

test("composeCommand explains when neither implementation is available", () => {
  assert.throws(
    () =>
      composeCommand({}, () => {
        throw new Error("unavailable");
      }),
    /neither docker nor podman compose is available/,
  );
});
