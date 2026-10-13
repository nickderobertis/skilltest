# Workflow contracts

`macos-policy.mjs classify --event PATH --base REF --head REF` is the shared
release-only authority. Production passes GitHub's event name in the environment;
fixtures include `event_name`. Author and title confer no authority. The configured
same-repository release-plz prefix plus parsed version/changelog-only content
permits omission; unreadable PR data requires the Mac lane.

CI spends Mac only on ordinary PRs. Its Linux `check (macos-latest)` carrier
reports the actual prerequisite results, including authorized omission. Preserve
all four protected display names. The tag build spends one Apple Silicon job on
the only Darwin target (arm64; Intel macOS is discontinued) and its native
installation proofs; publishing and archive transport
use Linux. The explicit diagnostic input defaults off and never publishes.

External reusable workflow fixtures are immutable upstream bytes, hash-locked in
`reusable-workflows.json`; refresh the lock and bytes together when the caller
changes. Dynamic screencomp outputs must come from its real reader. Unknown
calls, runners and expressions are errors. Recording tests disable Nx caching;
budget reporters consume existing digest-bound telemetry and never rerun scenarios.

The native packaging journey belongs to release-packaging's explicit packaging
edge. Foreign architecture evidence is a content contract; only native runners
can prove execution.
