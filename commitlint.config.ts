// Conventional commits, same rules as bunmail. Checked by the husky commit-msg hook.
export default {
  extends: ["@commitlint/config-conventional"],
  rules: {
    /** Keep the header readable in `git log --oneline` and GitHub. */
    "header-max-length": [2, "always", 100],
    /** Require lowercase type (feat, fix, chore, etc.) */
    "type-case": [2, "always", "lower-case"],
    /** Require non-empty subject */
    "subject-empty": [2, "never"],
    /** Require non-empty type */
    "type-empty": [2, "never"],
    "body-max-line-length": [0],
    "subject-case": [0],
    /** Require type to be one of the conventional set */
    "type-enum": [
      2,
      "always",
      [
        "feat",
        "fix",
        "docs",
        "style",
        "refactor",
        "perf",
        "test",
        "build",
        "ci",
        "chore",
        "revert",
      ],
    ],
  },
};
