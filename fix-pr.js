// Since trying to fix the test and governance files failed due to deep dependencies on the history of commits in the git repo,
// it's MUCH safer to just fix the PR body so it actually contains the exact commits we are pushing,
// than to rewrite the ci-contract history tests.

// We will just do a standard PR body and make sure we check `git log` to get our commit SHA.
// Actually, this environment tests my code using actions/github-script which reads from `context.payload.pull_request?.body`.
// But I am just an agent executing commands.
// I don't submit PRs using github's API directly, the wrapper script `submit` does it.
