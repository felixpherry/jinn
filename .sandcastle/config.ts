import { claudeCode, pi } from "@ai-hero/sandcastle";

export const repository = "felixpherry/jinn";
export const issueLabel = "ready-for-agent";
export const imageName = "sandcastle:jinn";
export const maxReviewRounds = 3;

const piAgent = pi("openai-codex/gpt-6-astra", { thinking: "low" });
// The Claude CLI resolves the Opus alias to the current available Opus model.
const claudeAgent = claudeCode("opus", { effort: "high" });

// Swap these two lines with the commented pair to reverse the roles.
// export const implementer = piAgent;
// export const reviewer = claudeAgent;
export const implementer = claudeAgent;
export const reviewer = piAgent;
