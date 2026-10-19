# Security policy

ortu handles personal data. A vulnerability in it can expose real people, so security reports are handled before any other work.

## Supported versions

ortu has not had a release yet. Until v1.0.0, only the latest commit on `main` receives fixes. This table will list supported release lines once they exist.

## Reporting a vulnerability

**Do not open a public issue.** Report privately through GitHub's private vulnerability reporting:

<https://github.com/aral-by/ortu/security/advisories/new>

Please include:

- the affected commit or version;
- a description of the issue and its impact;
- a minimal reproduction using **synthetic data only**. Never include real personal data in a report, even if that is how you found the issue.

You can expect an acknowledgement within five business days, followed by an assessment and a fix timeline. Once a fix is released, the advisory is published with credit to the reporter, unless they prefer otherwise.

## In scope

Issues that break the guarantees in [docs/systemdesign.md](docs/systemdesign.md) and [docs/data-flow.md](docs/data-flow.md), for example:

- a raw value reaching logs, metrics, audit events, Realtime output or any persisted output;
- a bypass of the tool-argument provenance check, or rehydration returning a value other than the one tokenized;
- token reversal without the key, or cross-call linking under call scope;
- weaknesses in vault encryption, AAD binding, key handling or zeroization;
- a fail-closed path that releases text.

## Out of scope

- Detection misses (recall gaps) without a security impact: please open a regular issue with a synthetic example.
- Data exposure that happens before ortu receives the data, such as audio sent to a cloud STT provider.
