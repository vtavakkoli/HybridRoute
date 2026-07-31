# Security Policy

## Status

HybridRoute is an early-stage research and engineering project. It must be reviewed and tested in the target environment before production use.

## Reporting a vulnerability

Please use GitHub private vulnerability reporting for this repository when it is enabled. Do not disclose suspected vulnerabilities in a public issue.

Include the affected version, reproduction steps, expected and actual behavior, impact, and any suggested remediation.

## Deployment guidance

- Authenticate callers before trusting role or tenant headers.
- Remove client-supplied routing-decision headers at the edge.
- Keep high-impact routes out of probabilistic selection.
- Use clarification or fallback for ambiguous payments, deletions, medical actions, identity changes, and legal submissions.
- Restrict upstream networks so clients cannot bypass HybridRoute.
- Use TLS and verify certificates for remote embedding services and upstream APIs.
- Store API keys in a secret manager, not in configuration files.
- Set strict request-size, timeout, and rate limits.
- Treat request text and embeddings as potentially sensitive data.
