# ADR-0028: External adoption evidence before broad product claims

- Status: Accepted
- Date: 2026-07-24
- Deciders: MindLeak maintainers
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence before
  completion), [ADR-0010](0010-observability-and-resilience.md) (measured
  behavior), [ADR-0016](0016-platform-packaging-and-registration.md) (public
  distribution), [EVALUATION.md](../EVALUATION.md)

## Context

MindLeak has credible internal evidence: deterministic truth-set benchmarks,
passive-sensor latency measurements, a pinned Extension Host smoke, and one
controlled real-agent scenario where MindLeak/Lodestar crossed the declared
productization gate. Those results justify shipping and further investment.

They do not establish that an independent developer can install the product,
understand it, keep it enabled, or receive useful context in a different
repository/model/workflow. Dogfooding is unusually deep but comes from the
author and the repository that shaped the product. Stars, downloads, and a green
CI run measure distribution/activity, not retained user value.

Without an explicit evidence policy, narrow benchmark percentages can harden
into broad marketing claims, or the project can react to a handful of anecdotes
by expanding languages and features before learning why users stay or leave.

## Decision

Separate product evidence into three named tiers and constrain claims to the
highest tier actually reached.

| Tier | Evidence | Permitted claim |
|---|---|---|
| **Engineering** | unit/integration tests, deterministic fixtures, build/CI, packaging smoke | the declared behavior works on the tested surface |
| **Controlled efficacy** | pre-registered agent/retrieval experiments with controls and machine-readable artifacts | the measured cohort/scenario improved by the observed amount |
| **External adoption** | independent developers install and use released builds on real repositories over time | those observed users adopted, retained, and reported the measured outcomes |

Passing one tier never implies the next. Public documentation labels the tier,
sample, model/client, repository shape, and limitations beside any result.

### v0.1.1 external pilot

After the v0.1.1 release, run the existing Lodestar pilot task with these minimum
conditions:

- recruit 3-5 developers who already use multiple coding agents;
- installation occurs from public release assets without live setup assistance;
- at least two participants use MindLeak on real work for seven days;
- record platform/client, install success and elapsed time, day-1/day-7 use,
  resumed-task outcomes, duplicate/collision observations, useful versus
  irrelevant context, degraded capabilities, and disable/removal reasons; and
- publish anonymized aggregate results, failures, and limitations in
  `docs/EVALUATION.md` plus a machine-readable `benchmarks/results` artifact.

This is a learning gate, not a requirement to produce a positive result. A
failed or low-retention pilot is published and changes the roadmap before claims
are broadened.

### Privacy and consent

The pilot is explicit opt-in. Do not collect source, prompts, terminal output,
commands, graph databases, task text, or raw MCP traces centrally. Participants
may provide reviewed, redacted examples deliberately. Aggregate metrics are
derived locally or reported by the participant and stored without personal or
repository identifiers.

MindLeak does not add default product analytics to satisfy this ADR. Any future
telemetry proposal requires a separate privacy/security decision.

### Claim and roadmap discipline

- Current benchmark percentages remain scoped to their exact experiment.
- "Validated for multi-agent developers" requires external-adoption evidence;
  installability alone is insufficient.
- Language/parser expansion requires an observed pilot/repository need or a
  separately justified objective with fixture-backed acceptance criteria.
- Marketplace publication, download counts, stars, and issue activity are
  acquisition signals, not proof of user value.
- Product expansion follows the dominant observed friction after onboarding and
  coordination workflows are complete; the project does not optimize for feature
  count before retention is understood.

## Rejected alternatives

- **Treat internal dogfood as external validation:** author familiarity removes
  the onboarding and mental-model barriers the pilot must measure.
- **Use stars/downloads as the adoption gate:** they do not show successful
  installation, continued use, or useful outcomes.
- **Enable automatic analytics by default:** conflicts with the local/privacy
  posture and is unnecessary for a small deliberate pilot.
- **Wait for statistically universal proof before releasing:** prevents learning;
  preview/stable releases can continue with accurately scoped claims.
- **Publish only successful cases:** launders selection bias and prevents roadmap
  correction.

## Consequences

- The v0.1.1 release and independent-user pilot form one ordered productization
  chain; the pilot does not block shipping the release.
- Evaluation artifacts become the source for product claims, with explicit tier
  and scope.
- Weak retention or onboarding failure is actionable evidence, not a reason to
  reinterpret the existing benchmark.
- Broad parser, marketplace, or hosted-service investment waits for observed
  demand rather than architectural enthusiasm.
