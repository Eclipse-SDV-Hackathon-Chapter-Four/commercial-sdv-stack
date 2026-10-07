<!--
SPDX-FileCopyrightText: Copyright (c) 2026 Contributors to the Eclipse Foundation

See the NOTICE file(s) distributed with this work for additional
information regarding copyright ownership.

This program and the accompanying materials are made available under the
terms of the Apache License Version 2.0 which is available at
https://www.apache.org/licenses/LICENSE-2.0

SPDX-License-Identifier: Apache-2.0
-->

<!-- this readme was generated with the help of AI -->

# SPIRE Workload Registration

Workloads are registered with the SPIRE server by running (from the project root):

```bash
scripts/register_workloads.sh
```

The script approves the *locally built* workload images: it generates
`config/spire/approved-workloads.list` (not tracked in git) with the current
image config digests and registers them with the SPIRE server. This simulates
the approval step that an OEM's release pipeline would perform (and sign) at
image release time in a real deployment.

SPIRE agents then only issue SVIDs to containers whose image config digest
matches an approved entry — a workload running an unapproved (e.g. tampered
or outdated) image is denied an identity.

> **Note:** SPIRE attests the digest of the image a container was *started
> from*. After rebuilding a workload image, recreate its container
> (`docker compose up -d <service>`) **before** running the registration
> script; otherwise the running container keeps the old digest, no longer
> matches the approved list, and is denied an SVID. The script prints a
> warning when it detects such a mismatch. Conversely, re-running the
> registration script after an *unapproved* image change is exactly how a
> tampered workload would be re-admitted — in production this decision
> belongs to the release pipeline, not the deployment host.

Currently registered entries can be listed with:

```bash
scripts/show_registered_workloads.sh
```
