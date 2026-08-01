# Security Policy

## Project status

HybridRoute is an early-stage research and engineering project. It is not a substitute for authentication, authorization, network policy, API governance, or application-specific safety review. Test and review the complete deployment in its target environment before production use.

## Supported versions

Security fixes are applied to the latest development line.

| Version | Supported |
|---|---|
| `0.2.x` | Yes |
| `0.1.x` | No |

The project may revise this policy as it approaches a stable `1.0` release.

## Reporting a vulnerability

Do not disclose suspected vulnerabilities in a public issue, discussion, pull request, benchmark output, or log excerpt.

Use GitHub private vulnerability reporting for this repository when available. Include:

- affected version or commit;
- deployment mode and relevant configuration;
- concise impact assessment;
- reproduction steps or a minimal proof of concept;
- expected and actual behavior;
- whether credentials, personal data, or route-policy boundaries are involved;
- suggested remediation, when known.

Avoid including real secrets, tokens, personal data, or production request bodies. Replace them with safe test values.

A maintainer will acknowledge a complete report, assess severity and scope, coordinate remediation, and publish an advisory when appropriate. Response timing depends on report complexity and maintainer availability; the project does not currently provide a formal security SLA.

## Security model

HybridRoute assumes a trusted edge or identity layer has already authenticated callers and established security-sensitive request context.

The router enforces configuration and selection invariants, including:

- policy and schema filtering before semantic ranking;
- high-impact route exclusion from exploration and online adaptation;
- fallback route exclusion from online adaptation;
- a maximum of one configured fallback route;
- atomic publication of validated immutable route-table generations;
- health and circuit eligibility before normal route selection;
- clarification or fallback when configured thresholds do not justify a confident route.

These controls reduce routing risk but do not prove that a target API is safe or that an authenticated caller is authorized to perform the target operation.

## Deployment guidance

### Identity and policy headers

- Authenticate callers before trusting role, tenant, identity, domain, or data-classification headers.
- Strip client-supplied security-sensitive routing headers at the external edge.
- Populate trusted headers only after authentication and authorization.
- Use route-level `required_roles`, `forbidden_roles`, and `required_headers` as defense in depth, not as the sole identity system.

### Network boundaries

- Restrict upstream networks so callers cannot bypass HybridRoute.
- Restrict HybridRoute egress to approved upstream and embedding endpoints.
- Use TLS for remote upstreams and embedding services, and verify certificates.
- Apply ingress rate limits, body-size limits, connection limits, and timeouts.
- Run the container as a non-root user and retain the read-only configuration mount used by the example deployment.

### Administration endpoints

- Replace the demonstration administration token in `docker-compose.yml`.
- Store tokens and API keys in a secret manager, not in source-controlled TOML or Compose files.
- Limit access to `/v1/admin/reload` and `/v1/feedback` at the network and gateway layers.
- Rotate administration and embedding credentials after suspected exposure.
- Monitor feedback submissions because accepted feedback can influence operational route quality.

### High-impact operations

Use explicit policies and human review for requests involving areas such as:

- payments or financial transfers;
- destructive deletion;
- identity or entitlement changes;
- medical actions;
- legal or regulatory submissions;
- safety-critical infrastructure controls.

For these routes:

```toml
high_impact = true
safe_for_exploration = false
allow_adaptation = false
```

Prefer clarification or a deterministic fallback over ambiguous automatic execution.

### Request and embedding privacy

- Treat request text, extracted fields, embeddings, candidate lists, and feedback as potentially sensitive.
- Minimize logging of request bodies and semantic text.
- Review data residency and retention when using a remote embedding endpoint.
- Do not send secrets or unnecessary personal data to an embedding service.
- Protect metrics and status endpoints when route names or operational state are sensitive.

### Proxy behavior

- Review forwarded headers and upstream trust assumptions.
- Remove internal decision headers at external boundaries.
- Validate whether preserving the original query string is appropriate for each deployment.
- Apply upstream-specific authorization even after a route is selected.
- Treat upstream 5xx responses and connection failures as operational signals, not proof of malicious behavior.

## Threats outside the router's scope

HybridRoute does not independently prevent:

- compromised upstream APIs;
- malicious or incorrect identity-provider assertions;
- prompt injection inside downstream AI systems;
- business-logic authorization flaws;
- data exfiltration through an approved upstream;
- denial of service without external rate and resource controls;
- unsafe route definitions approved by an operator;
- dependency or build-pipeline compromise.

Use dependency review, locked build inputs, container scanning, infrastructure policy, and application-level authorization as part of the complete system.

## Security-related changes

Changes to policy eligibility, ambiguity handling, high-impact behavior, administration endpoints, forwarded headers, credential handling, schema filtering, or operational adaptation require explicit security discussion in the pull request.
