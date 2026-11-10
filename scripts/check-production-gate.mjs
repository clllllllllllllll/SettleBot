export function requireReviewers(environment) {
  const rules = environment?.protection_rules;
  if (!Array.isArray(rules) || !rules.some(requiredReviewers)) {
    throw new Error("Production must have required reviewers before delivery is enabled");
  }
}

function requiredReviewers(rule) {
  return rule?.type === "required_reviewers" && Array.isArray(rule.reviewers)
    && rule.reviewers.some(entry => ["User", "Team"].includes(entry?.type)
      && Number.isSafeInteger(entry?.reviewer?.id) && entry.reviewer.id > 0);
}

function requireSelectedBranches(environment) {
  const policy = environment?.deployment_branch_policy;
  if (policy?.protected_branches !== false || policy.custom_branch_policies !== true) {
    throw new Error("Environment must use explicit selected branch policies");
  }
}

function requireMainOnly(policies) {
  const rules = policies?.branch_policies;
  if (policies?.total_count !== 1 || !Array.isArray(rules) || rules.length !== 1
    || rules[0]?.name !== "main" || rules[0]?.type !== "branch") {
    throw new Error("Environment must allow exactly the main branch, without tag policies");
  }
}

async function readGitHub(repository, path, token, fetcher) {
  const response = await fetcher(`https://api.github.com/repos/${repository}/environments/${path}`, {
    method: "GET", headers: { authorization: `Bearer ${token}`, accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28" },
    redirect: "error", signal: AbortSignal.timeout(10_000),
  });
  if (response.status !== 200) throw new Error("Cannot verify environment protection");
  return response.json();
}

export async function verifyProductionGate(repository, token, fetcher = fetch) {
  if (!/^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/.test(repository ?? "") || !token) {
    throw new Error("Repository and environment-read token required");
  }
  for (const target of ["production", "staging"]) {
    const environment = await readGitHub(repository, target, token, fetcher);
    if (target === "production") {
      requireReviewers(environment);
      // Omitted from GitHub's OpenAPI environment schema; require the explicit response field.
      // GitHub's own provider: integrations/terraform-provider-github, repository_environment.
      if (environment.can_admins_bypass !== false) throw new Error("Administrator bypass must be disabled");
    }
    requireSelectedBranches(environment);
    const policies = await readGitHub(repository, `${target}/deployment-branch-policies?per_page=100&page=1`, token, fetcher);
    requireMainOnly(policies);
  }
}

if (import.meta.main) {
  try {
    await verifyProductionGate(process.env.GITHUB_REPOSITORY, process.env.GH_TOKEN);
  } catch {
    console.error("Production protection could not be verified; refusing deployment");
    process.exitCode = 1;
  }
}
