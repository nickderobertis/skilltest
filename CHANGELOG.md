# Changelog

## [0.9.0](https://github.com/nickderobertis/skilltest/compare/v0.8.0...v0.9.0) (2026-07-07)

### Features

* record oneharness run history and surface a per-run review command ([#29](https://github.com/nickderobertis/skilltest/issues/29)) ([49afc94](https://github.com/nickderobertis/skilltest/commit/49afc94dc0c253dcab46293ad73a91447443a1f0))

### Documentation

* **screenshots:** add terminal screenshot + diffing (screencomp) setup ([#28](https://github.com/nickderobertis/skilltest/issues/28)) ([8d1854d](https://github.com/nickderobertis/skilltest/commit/8d1854ddc42ca92027136340d321b100518da065))

## [0.8.0](https://github.com/nickderobertis/skilltest/compare/v0.7.0...v0.8.0) (2026-07-07)

### ⚠ BREAKING CHANGES

* skilltest-core's Error::Provider.kind is now Option<ProviderErrorKind> (was Option<String>) and Error::provider_classified takes a ProviderErrorKind. The JSON output contract is only extended, so SDK/plugin consumers are unaffected.

### Features

* structured provider errors with a classified kind across the CLI and SDKs ([#27](https://github.com/nickderobertis/skilltest/issues/27)) ([6ae7a1f](https://github.com/nickderobertis/skilltest/commit/6ae7a1f69cd92aabb070cc29de0940f48086a730))

## [0.7.0](https://github.com/nickderobertis/skilltest/compare/v0.6.0...v0.7.0) (2026-07-07)

### Features

* reference mocks/spies by object in called/not_called evals ([#26](https://github.com/nickderobertis/skilltest/issues/26)) ([8da0bc1](https://github.com/nickderobertis/skilltest/commit/8da0bc1a0f95ffd07447e342ae8187dac7ccac03))

## [0.6.0](https://github.com/nickderobertis/skilltest/compare/v0.5.0...v0.6.0) (2026-07-07)

### Features

* define full test cases in code via the SDKs and plugins ([#25](https://github.com/nickderobertis/skilltest/issues/25)) ([67208ca](https://github.com/nickderobertis/skilltest/commit/67208cad386a3ae87a1c8a194678514d40ba5fe7))

## [0.5.0](https://github.com/nickderobertis/skilltest/compare/v0.4.0...v0.5.0) (2026-07-06)

### Features

* tool mocking and spying — stub/deny/rewrite/spy across YAML, SDKs, and plugins ([#24](https://github.com/nickderobertis/skilltest/issues/24)) ([1902b42](https://github.com/nickderobertis/skilltest/commit/1902b42c3b9fefde3b232f1f206fb089ae1e1207))

### Documentation

* document tool events + streaming API in the READMEs ([#23](https://github.com/nickderobertis/skilltest/issues/23)) ([60733ab](https://github.com/nickderobertis/skilltest/commit/60733ab556459f6ec0c80750cdecca838eb755b3))

## [0.4.0](https://github.com/nickderobertis/skilltest/compare/v0.3.0...v0.4.0) (2026-07-06)

### ⚠ BREAKING CHANGES

* skilltest now targets oneharness v0.3.6 and passes no --mode, so oneharness's normalized default approval mode applies (v0.3.0+) instead of pre-0.3 allow-everything. Set ONEHARNESS_MODE=bypass via oneharness config to restore the prior behavior.

### Features

* normalized tool events + opt-in streaming API (oneharness v0.3.6) ([#22](https://github.com/nickderobertis/skilltest/issues/22)) ([1a2fb6b](https://github.com/nickderobertis/skilltest/commit/1a2fb6b2ce525885fed20306a8526a20be99861d))

## [0.3.0](https://github.com/nickderobertis/skilltest/compare/v0.2.2...v0.3.0) (2026-06-12)

### Features

* direct-API judge backend (Anthropic + OpenAI) with strict JSON ([#17](https://github.com/nickderobertis/skilltest/issues/17)) ([2a22269](https://github.com/nickderobertis/skilltest/commit/2a22269b5d50e81a904f27254f8143591af9740b))

## [0.2.2](https://github.com/nickderobertis/skilltest/compare/v0.2.1...v0.2.2) (2026-06-12)

### Bug Fixes

* commit the platform package version bumps on release ([#18](https://github.com/nickderobertis/skilltest/issues/18)) ([572744a](https://github.com/nickderobertis/skilltest/commit/572744a4c54d12bc32867767d107d3780a9e55dd))

## [0.2.1](https://github.com/nickderobertis/skilltest/compare/v0.2.0...v0.2.1) (2026-06-12)

### Bug Fixes

* absolutize out-dir in the python dist scripts ([#16](https://github.com/nickderobertis/skilltest/issues/16)) ([9ca8f47](https://github.com/nickderobertis/skilltest/commit/9ca8f47f1e64ca69af02aac4c6dc6738eb21d489))

## [0.2.0](https://github.com/nickderobertis/skilltest/compare/v0.1.1...v0.2.0) (2026-06-12)

### Features

* bundle the CLI in the SDKs so install needs no separate binary ([#13](https://github.com/nickderobertis/skilltest/issues/13)) ([2f6b0f8](https://github.com/nickderobertis/skilltest/commit/2f6b0f8fc6d19ecde5df74fd019f464e6c3d2d39))

## [0.1.1](https://github.com/nickderobertis/skilltest/compare/v0.1.0...v0.1.1) (2026-06-12)

### Bug Fixes

* publish npm packages under the [@skill-test](https://github.com/skill-test) scope ([#11](https://github.com/nickderobertis/skilltest/issues/11)) ([432e9f1](https://github.com/nickderobertis/skilltest/commit/432e9f1b8b2ee4329a4739559220553cabdd9d5a))

## [0.1.0](https://github.com/nickderobertis/skilltest/compare/v0.0.0...v0.1.0) (2026-06-12)

### Features

* lockstep versioning driven by semantic-release ([#10](https://github.com/nickderobertis/skilltest/issues/10)) ([6fc5253](https://github.com/nickderobertis/skilltest/commit/6fc5253e3c2296c6d6de8b780f8caaf1a5d9e1c2))
