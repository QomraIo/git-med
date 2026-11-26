# git-med login

## How to use
`git-med login --server <server_url> --username <username> --password <password>`

## What
Authenticates a user against a `git-med-forge` server instance (or Auth0 SSO provider), obtains a Bearer access token, and stores it in the local credentials file.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Purpose | Authenticates CLI with remote forge server and persists access token | Handled by credential helpers (`git credential`) or GitHub CLI (`gh auth login`) | Built directly into core `git-med` CLI without requiring external helper tools. |
| Storage Location | `~/.git-med/credentials` (or `$GIT_MED_CREDENTIALS_FILE`) | `~/.git-credentials`, system keychain, or cache daemon | Dedicated origin-keyed JSON credential configuration file. |
| Token Type | JWT Bearer access token | Basic authentication password or Personal Access Token (PAT) | Modern OAuth2/JWT token exchange directly supported by server. |
| Non-Interactive Support | Accepts `--username` and `--password` CLI flags or prompts securely via stdin | Accepts interactive prompts or helper stdin protocol | Supports both interactive and automated/headless authentication flows. |
