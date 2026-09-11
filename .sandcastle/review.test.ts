import { test } from "node:test";
import assert from "node:assert/strict";
import { requireApproval, reviewApproved } from "./review.ts";

for (const [name, document, expected] of [
  ["approves an empty findings list with explicit approval", '{"approved":true,"findings":[]}', true],
  ["rejects approval with unresolved findings", '{"approved":true,"findings":["bug"]}', false],
  ["rejects requested changes", '{"approved":false,"findings":[]}', false],
  ["rejects malformed review output", "not json", false],
  ["rejects missing findings", '{"approved":true}', false],
] as const) {
  test(name, () => {
    // Given a review document.
    // When deciding whether it approves the implementation.
    const approved = reviewApproved(document);
    // Then only explicit approval without findings passes.
    assert.equal(approved, expected);
  });
}

test("accepts work when review and verification pass", async () => {
  // Given an independently approved and verified implementation.
  const cycle = {
    review: async () => ({ approved: true, feedback: "" }),
    verify: async () => ({ passed: true, feedback: "" }),
    fix: async () => { throw new Error("No fix should be required"); },
  };
  // When applying the merge gate.
  // Then the gate permits continuation.
  await assert.doesNotReject(requireApproval(cycle, 3));
});

for (const [name, approved, passed] of [
  ["blocks merge when verification keeps failing despite approval", true, false],
  ["blocks merge when review keeps failing despite passing tests", false, true],
] as const) {
  test(name, async () => {
    // Given a persistently failing acceptance condition.
    const cycle = {
      review: async () => ({ approved, feedback: "review" }),
      verify: async () => ({ passed, feedback: "tests" }),
      fix: async () => {},
    };
    // When three review/fix rounds are exhausted.
    // Then merge is blocked.
    await assert.rejects(requireApproval(cycle, 3), /failed after 3 rounds/);
  });
}

test("sends review and test feedback to the implementer for correction", async () => {
  // Given a failed first round followed by a successful correction.
  const feedback: string[] = [];
  const cycle = {
    review: async (round: number) => ({ approved: round > 1, feedback: "review finding" }),
    verify: async (round: number) => ({ passed: round > 1, feedback: "test failure" }),
    fix: async (message: string) => { feedback.push(message); },
  };
  // When the correction cycle runs.
  await requireApproval(cycle, 3);
  // Then both sources of feedback reach the implementer together.
  assert.deepEqual(feedback, ["review finding\n\ntest failure"]);
});
