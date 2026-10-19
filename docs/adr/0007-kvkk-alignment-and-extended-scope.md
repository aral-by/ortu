# ADR-0007: KVKK alignment and extended scope

- **Status:** Accepted
- **Date:** 2026-10-20
- **Affects:** systemdesign.md sections 2.1, 3.4, 17 and 20; README

## Context

Reviewing realistic call content showed what the M0-M3 plan does not cover. In *"Ben buranın muhtarı Mehmet Yılmaz, annem kemoterapi gören kadın, anneme personeller çok kötü davranmış, kadın bayılmış"*, nothing is masked today: the name has no introducer, "buranın muhtarı" identifies one person, and the mother's treatment is special-category health data. Calls also carry identifiers the plan never listed: one-time codes read from SMS, a mother's maiden name or father's name for identity checks, subscriber numbers, ID card serial numbers.

Writing a rule for every category of personal data would never end, and some categories (health, religion, indirect identifiers) are about meaning rather than form.

## Options considered

1. **Promise complete coverage.** Unachievable; would make every document wrong.
2. **Keep the original scope.** Leaves cheap, high-value cases (one-time codes, health terms) undone.
3. **State what ortu is, extend it where rules work, and name its limits.**

## Decision

Option 3.

- ortu is a **data-minimization layer**, mapped explicitly to the KVKK principles and duties it supports (systemdesign.md §3.4), never a compliance guarantee.
- A new milestone **M4b** adds the identifiers rules handle well (one-time codes and PINs, parents' names, ID card serials, IMEI, VIN, cadastral parcels, amounts after a personal cue, configurable custom identifiers), a **health term lexicon** with a per-tenant special-category policy, and a **unique-role flag** for indirect identifiers.
- Model-based detection of special categories follows the M7 evaluation, in the order: pretrained NER, a local classifier in PostCall mode, then a trained model once labeled data exists. Nothing model-based runs in the Realtime path.
- The limits are written down: content an assistant must understand reaches the LLM, so such services need an in-country or self-hosted LLM; indirect identifiers need dataset-level techniques; audio sent to cloud STT is out of reach; legal duties stay with the controller.

## Rationale

Rules are exact and cheap where data has form (codes, checksums, cues); they fail where data is meaning. Spending effort where rules work, and being explicit where they do not, keeps the project finishable and its claims true. The infrastructure built in M1 and M2 makes each additional structured kind a small, mostly data-driven change.

## Consequences

- Use cases gain pseudonymized analytics, the right to erasure and audited access, each tied to the milestone that delivers it.
- Open question 9 asks for the default special-category policy (`mask_sentence`, `persist_only` or `flag`).
- M4b formats (ID card serials, driving licences, social security numbers, VIN) are verified against official sources before acceptance, as the area codes were.
