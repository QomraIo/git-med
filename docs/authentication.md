# Authentication in git-med

This document provides a comprehensive guide to authentication and authorization in the `git-med` client ecosystem and its companion server, `git-med-forge`.

---

## 1. Overview and Architecture

`git-med` uses a token-based authentication model to authorize access to remote repositories hosted on `git-med-forge`.

When performing network operations such as `git-med clone`, `git-med push`, or `git-med pull`, the CLI automatically resolves an access token for the target server and transmits it via the standard HTTP `Authorization: Bearer <token>` header.

```
+------------------+                    +---------------------+
|   git-med CLI    |                    |   git-med-forge     |
|                  |                    |    (Server)         |
+--------+---------+                    +----------+----------+
         |                                         |
         |  1. POST /api/v1/auth/login             |
         |     {username, password, server}        |
         +---------------------------------------->|
         |                                         |  Validates against
         |                                         |  DB or Auth0 JWKS
         |  2. Returns JWT Bearer Token            |
         |<----------------------------------------+
         |
         |  3. Saves token to ~/.git-med/credentials
         |
         |  4. git-med clone / push / pull
         |     Headers: "Authorization: Bearer <token>"
         +---------------------------------------->|
         |                                         |  Enforces RBAC:
         |                                         |  Public / Protected / Private
         |  5. Authorized response + LFS transfers |
         |<----------------------------------------+
```

---

## 2. Supported Authentication Providers

### A. Local User Authentication
`git-med-forge` supports built-in username and password authentication backed by its local database (SQLite/PostgreSQL):
* The endpoint `/api/v1/auth/login` validates credentials.
* On success, the server responds with a signed JWT access token (`token_type: "Bearer"`).

### B. Auth0 / OIDC SSO Integration
When Auth0 is enabled on `git-med-forge`:
* User accounts are managed via Auth0 databases and social connections.
* Access tokens are signed with RS256 by the Auth0 tenant.
* `git-med-forge` validates tokens using Auth0's JSON Web Key Set (`/.well-known/jwks.json`).
* If Auth0 is unavailable or unconfigured, the server gracefully falls back to local authentication mode.

---

## 3. Credential Storage

### Credential File Location
By default, `git-med` stores credentials in a JSON file at:
```bash
~/.git-med/credentials
```
This path can be overridden by setting the `GIT_MED_CREDENTIALS_FILE` environment variable.

### File Schema
The credential store maps normalized server origins to stored credentials:

```json
{
  "servers": {
    "http://localhost:3000": {
      "username": "alice",
      "token": "eyJhbGciOiJSUzI1NiIsInR5cCI6...",
      "token_type": "Bearer",
      "created_at": "2026-11-21T17:30:00Z"
    },
    "https://forge.example.com": {
      "username": "researcher1",
      "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6...",
      "token_type": "Bearer",
      "created_at": "2026-11-21T16:00:00Z"
    }
  }
}
```

### Origin Normalization and Matching
When making a request to a remote URL (e.g., `http://localhost:3000/alice/brain-mri.git`), `git-med` extracts the origin (`http://localhost:3000`) and resolves the token using the following precedence:
1. Environment variables (`GIT_MED_TOKEN` or `GIT_MED_AUTH_TOKEN`)
2. Exact server origin match in `~/.git-med/credentials`
3. Normalized trailing-slash origin match (`http://localhost:3000/`)
4. Prefix match on the configured server URL

---

## 4. Environment Variables

The CLI and test harnesses support several environment variables for authentication and configuration:

| Variable | Description |
| --- | --- |
| `GIT_MED_TOKEN` | Direct Bearer token override for all remote operations (highest precedence). |
| `GIT_MED_AUTH_TOKEN` | Alternative fallback token override (same precedence as `GIT_MED_TOKEN`). |
| `GIT_MED_CREDENTIALS_FILE` | Custom file path for the credential store instead of `~/.git-med/credentials`. |
| `AUTH0_DOMAIN` | Auth0 tenant domain for live SSO verification (server-side). |
| `AUTH0_CLIENT_ID` | Auth0 application client identifier. |
| `AUTH0_CLIENT_SECRET` | Auth0 client secret used for server-side token validation. |

---

## 5. CLI Authentication Commands

### Login
Authenticate with a remote server and save credentials:
```bash
git-med login --server http://localhost:3000 --username alice --password secret
```
If `--username` or `--password` are omitted, `git-med` prompts interactively (with hidden password input).

### Whoami
Inspect the currently authenticated user for a given server:
```bash
git-med whoami --server http://localhost:3000
```
Output:
```text
Logged in as: alice (http://localhost:3000)
```

### Logout
Clear stored credentials for a specific server:
```bash
git-med logout --server http://localhost:3000
```
Output:
```text
Logged out from http://localhost:3000
```

---

## 6. Repository Authorization and Access Control (RBAC)

`git-med-forge` implements Role-Based Access Control (RBAC) across three repository visibility tiers:

| Visibility | Read / Clone Access | Write / Push Access |
| --- | --- | --- |
| **Public** | Open to all users (anonymous and authenticated). | Restricted to repository owner and authorized collaborators. |
| **Protected** | Readable by all authenticated users or team members. | Restricted to repository owner and maintainers. |
| **Private** | Restricted strictly to the repository owner and explicitly granted collaborators. | Restricted strictly to repository owner and collaborators with write permission. |

### Access Denial Behaviors
* **Anonymous Access to Private Repositories**: Returns HTTP 404 (Not Found) or HTTP 401 (Unauthorized) to prevent leaking repository existence.
* **Unauthorized Push**: Attempting to push to a repository without write permissions returns HTTP 403 (Forbidden).
* **Missing Token**: When cloning or pushing to a private repository without credentials, `git-med` exits with an authorization error instructing the user to run `git-med login`.
