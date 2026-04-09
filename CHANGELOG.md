# Changelog

## [0.3.0](https://github.com/mangas/mcpfile/compare/mcpfile-v0.2.3...mcpfile-v0.3.0) (2026-04-09)


### Features

* write VERSION file to S3 latest/ on publish ([95bf3c0](https://github.com/mangas/mcpfile/commit/95bf3c0418225d40db4ba0dca9f07250ba7e866e))


### Bug Fixes

* resolve Docker Desktop socket and pull images before run ([#14](https://github.com/mangas/mcpfile/issues/14)) ([2b846a8](https://github.com/mangas/mcpfile/commit/2b846a8631abfb14350c46724b74d26bea523956))
* resolve Docker Desktop socket on macOS ([#13](https://github.com/mangas/mcpfile/issues/13)) ([5464e0d](https://github.com/mangas/mcpfile/commit/5464e0db42168a3a946fdb15043845ef1b45f1d7))
* resolve Docker Desktop socket on macOS and pull images before run ([2b846a8](https://github.com/mangas/mcpfile/commit/2b846a8631abfb14350c46724b74d26bea523956))

## [0.2.3](https://github.com/mangas/mcpfile/compare/mcpfile-v0.2.2...mcpfile-v0.2.3) (2026-04-06)


### Bug Fixes

* add contents:write permission for GitHub Release upload ([148d5ab](https://github.com/mangas/mcpfile/commit/148d5ab862ff2b87e7904d1790bd8a631284aded))

## [0.2.2](https://github.com/mangas/mcpfile/compare/mcpfile-v0.2.1...mcpfile-v0.2.2) (2026-03-30)


### Bug Fixes

* install aws CLI on self-hosted runner before S3 upload ([0e39f7e](https://github.com/mangas/mcpfile/commit/0e39f7e0990fdc2cb9c9cbbf9ae7cab7e32bc92e))
* remove trailing blank lines in secrets.rs ([c08baad](https://github.com/mangas/mcpfile/commit/c08baadec083ccac706d146074d0afe7b990c901))

## [0.2.1](https://github.com/mangas/mcpfile/compare/mcpfile-v0.2.0...mcpfile-v0.2.1) (2026-03-30)


### Bug Fixes

* add Default impl for MockDockerClient ([135c876](https://github.com/mangas/mcpfile/commit/135c8768bf485f0bf0f87f3625b43ea87a71e0bf))

## [0.2.0](https://github.com/mangas/mcpfile/compare/mcpfile-v0.1.0...mcpfile-v0.2.0) (2026-03-30)


### Features

* add CI, release, Docker, dependabot, and justfile ([742ee1b](https://github.com/mangas/mcpfile/commit/742ee1b32702d329f100a20af349b26f42e24943))
* consolidate release into single manual workflow ([6f85232](https://github.com/mangas/mcpfile/commit/6f85232f15184ea3d4f7abf67064600eab819a95))
* enable static linking for all targets ([69f46db](https://github.com/mangas/mcpfile/commit/69f46dbbe1d6e82588071c008566e111b2f0c489))
* static musl binaries and scratch Docker image ([2e50141](https://github.com/mangas/mcpfile/commit/2e5014141d2752948214773dbdcbf356ee5e0199))


### Bug Fixes

* broaden CI/lint path triggers to all .github files ([e1fccac](https://github.com/mangas/mcpfile/commit/e1fccac8eff511f6b515919156abceb4a323fdc1))
* only set crt-static for musl target ([fb5b931](https://github.com/mangas/mcpfile/commit/fb5b9314334cf1564a43f16cd581a8a9c0e3348b))
* use arc-runner-set-mcpfile runner label ([30d29d8](https://github.com/mangas/mcpfile/commit/30d29d8ad3e30ef220b772d5f3a26a6a62ba7fd6))
