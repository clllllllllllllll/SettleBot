export function requireReviewers(environment) {
  const rules = environment.protection_rules ?? [];
  if (!rules.some((rule) => rule.type === "required_reviewers" && rule.reviewers?.length > 0)) {
    throw new Error("Production must have required reviewers before delivery is enabled");
  }
}

if (import.meta.main) {
  try {
    const repository = process.env.GITHUB_REPOSITORY;
    if (!/^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/.test(repository ?? "")) throw new Error("Repository required");
    const response = await fetch(`https://api.github.com/repos/${repository}/environments/production`, {
      headers: { authorization: `Bearer ${process.env.GH_TOKEN}`,
        accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" },
      redirect: "error", signal: AbortSignal.timeout(10_000),
    });
    if (!response.ok) throw new Error("Cannot verify production environment protection");
    requireReviewers(await response.json());
  } catch {
    console.error("Production protection could not be verified; refusing deployment");
    process.exitCode = 1;
  }
}
