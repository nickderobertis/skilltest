# Release packaging

The release-packaging project owns production staging and its artifact proofs.
Its slow packaging target stays off the ordinary affected gate: real native
builder/consumer journeys are selected explicitly through the release edge.
Foreign header fixtures prove format and payload contracts, never execution.
Keep archive/wheel/npm input bytes identical and exclude the fake provider from
shipped products; it belongs only to deterministic installation journeys.
