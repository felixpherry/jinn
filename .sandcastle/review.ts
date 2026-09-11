export interface ReviewCycle {
  review(round: number): Promise<{ approved: boolean; feedback: string }>;
  verify(round: number): Promise<{ passed: boolean; feedback: string }>;
  fix(feedback: string, round: number): Promise<void>;
}

/** Require both independent review and command-based verification before merging. */
export async function requireApproval(cycle: ReviewCycle, maxRounds: number): Promise<void> {
  for (let round = 1; round <= maxRounds; round++) {
    const review = await cycle.review(round);
    const verification = await cycle.verify(round);
    if (review.approved && verification.passed) return;
    if (round === maxRounds) break;
    await cycle.fix(`${review.feedback}\n\n${verification.feedback}`, round);
  }
  throw new Error(`Review or verification failed after ${maxRounds} rounds. Work preserved; nothing merged.`);
}

/** Fail closed on missing, malformed, or negative review verdicts. */
export function reviewApproved(document: string): boolean {
  try {
    const result: unknown = JSON.parse(document);
    return typeof result === "object" && result !== null &&
      "approved" in result && result.approved === true &&
      "findings" in result && Array.isArray(result.findings) && result.findings.length === 0;
  } catch {
    return false;
  }
}
