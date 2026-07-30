import test from "node:test";
import assert from "node:assert/strict";

// Importing must be free of side effects: this module reaches for `node:sqlite`
// and a live embeddings server, and CI pins Node 20 where the former does not
// exist. If either moves back to module scope, this import dies and the whole
// suite reports a failure that names this file rather than the cause.
import {
  cosine,
  fieldStats,
  separation,
  fromBlob,
} from "./evaluate-recall.mjs";

test("cosine is 1 for identical vectors and 0 for orthogonal ones", () => {
  assert.ok(Math.abs(cosine([1, 0, 0], [1, 0, 0]) - 1) < 1e-9);
  assert.equal(cosine([1, 0], [0, 1]), 0);
});

test("cosine refuses empty or mismatched vectors instead of guessing", () => {
  assert.equal(cosine([], []), 0);
  assert.equal(cosine([1, 0], [1, 0, 0]), 0);
});

test("fieldStats reports the top score's distance above its own field", () => {
  const stats = fieldStats([1, 1, 1, 5]);
  assert.equal(stats.top, 5);
  assert.equal(stats.mean, 2);
  assert.ok(stats.sigma > 1, "an outlier stands above the field it came from");
});

test("a field with no spread has no outliers, and says so rather than dividing by zero", () => {
  const stats = fieldStats([3, 3, 3, 3]);
  assert.equal(stats.sd, 0);
  assert.equal(stats.sigma, 0);
});

test("an empty field is answered, not crashed on", () => {
  assert.deepEqual(fieldStats([]), { mean: 0, sd: 0, top: 0, sigma: 0 });
});

test("separation finds a threshold when the bands are ordered", () => {
  const sep = separation([1, 2, 3], [4, 5, 6]);
  assert.equal(sep.separable, true);
  assert.equal(sep.gap, 1);
});

/// The finding this harness exists to test: overlapping bands admit no single
/// constant. Measured on the real index, nonsense reached 3.90 sigma while the
/// weakest real question reached 3.71 — so this is the case that actually held.
test("separation reports overlap rather than inventing a threshold", () => {
  const sep = separation([3.11, 3.73, 3.9], [3.71, 3.92, 6.21]);
  assert.equal(sep.separable, false);
  assert.ok(sep.gap < 0, "a negative margin is the honest answer");
});

test("fromBlob decodes the little-endian f32 vectors the index stores", () => {
  const source = Float32Array.from([1.5, -2.25, 0]);
  const bytes = new Uint8Array(source.buffer.slice(0));
  assert.deepEqual(Array.from(fromBlob(bytes)), [1.5, -2.25, 0]);
});
