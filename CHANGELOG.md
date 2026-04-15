# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## [v0.6.3](https://github.com/haymon-ai/database-mcp/compare/9f7aa3f88f43d033cf7f82545704dec88597a1a4..v0.6.3) - 2026-04-14
#### Features
- (**tools**) add display title to every MCP tool (#108) - ([c451823](https://github.com/haymon-ai/database-mcp/commit/c451823b9a2df550d346a7f650e4abffb8a5f2c3)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- (**install**) no-op when already on latest version (#106) (#109) - ([de58eaf](https://github.com/haymon-ai/database-mcp/commit/de58eafb218495b0848fb151a1c09371e21e4c32)) - [@athopen](https://github.com/athopen)
- (**tools**) omit input_schema for list_databases (#107) - ([46e19ba](https://github.com/haymon-ai/database-mcp/commit/46e19bafe2e7572dc4676d2e5f9babd776478b29)) - [@athopen](https://github.com/athopen)
#### Refactoring
- (**commands**) scope db args to transport subcommands (#103) - ([9f7aa3f](https://github.com/haymon-ai/database-mcp/commit/9f7aa3f88f43d033cf7f82545704dec88597a1a4)) - [@athopen](https://github.com/athopen)

- - -

## [v0.6.2](https://github.com/haymon-ai/database-mcp/compare/80f716f9f927484b61acbd0e7c78b6818c243955..v0.6.2) - 2026-04-12
#### Features
- (**install**) add one-liner install scripts for macOS, Linux, and Windows (#100) - ([b9a11d0](https://github.com/haymon-ai/database-mcp/commit/b9a11d0f47c1101d76deef4f5bd9e8b78d75fb12)) - [@athopen](https://github.com/athopen)
- (**release**) add macOS and Windows builds (#96) (#99) - ([04a17aa](https://github.com/haymon-ai/database-mcp/commit/04a17aa5060debd10f423e3e514b1748f6cca4e4)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- (**deps**) update rand 0.10.0 to 0.10.1 (RUSTSEC-2026-0097) (#92) - ([c5ba04d](https://github.com/haymon-ai/database-mcp/commit/c5ba04d6bb9d9df789eef2fbebb8a685bdab8ae0)) - [@athopen](https://github.com/athopen)
#### Documentation
- (**tools**) improve MCP tool descriptions with structured XML sections (#94) - ([330a822](https://github.com/haymon-ai/database-mcp/commit/330a822dcd4acabb22affe9d7cf9035157dbc038)) - [@athopen](https://github.com/athopen)
#### Refactoring
- (**server**) relocate Server wrapper to server crate (#88) - ([fe4a9e7](https://github.com/haymon-ai/database-mcp/commit/fe4a9e7c846e6c74ed70337fd015ba763a0b8630)) - [@athopen](https://github.com/athopen)
- (**sql**) unify connection management and add comprehensive tests (#89) - ([c894019](https://github.com/haymon-ai/database-mcp/commit/c894019b33624f3f4276cee0ec548bc92ad573c9)) - [@athopen](https://github.com/athopen)

- - -

## [v0.6.1](https://github.com/haymon-ai/database-mcp/compare/45f73a649ffce25cd1e17b3ff24e6b2c1d8971b8..v0.6.1) - 2026-04-10
#### Features
- (**ci**) automate release publishing to ghcr.io, MCP Registry, and crates.io (#86) - ([45f73a6](https://github.com/haymon-ai/database-mcp/commit/45f73a649ffce25cd1e17b3ff24e6b2c1d8971b8)) - [@athopen](https://github.com/athopen)
#### Documentation
- add code of conduct - ([7d8b740](https://github.com/haymon-ai/database-mcp/commit/7d8b740de8926d04461b8fc35d7f7c576ed4e3f5)) - [@athopen](https://github.com/athopen)
#### Refactoring
- (**handlers**) replace rmcp tool macros with per-tool ZSTs (#87) - ([024a4e9](https://github.com/haymon-ai/database-mcp/commit/024a4e902c30c5a39fa540c4b06716e99f644c3d)) - [@athopen](https://github.com/athopen)
- (**server**) drop unused From impls for ServerHandler - ([2860656](https://github.com/haymon-ai/database-mcp/commit/28606562977a48446bec1b534dd1950ad0e5dabd)) - athopen

- - -

## [v0.6.0](https://github.com/haymon-ai/database-mcp/compare/2e442034fb521424e209c3498b192dd55d328d9a..v0.6.0) - 2026-04-08
#### Features
- defer database connection via lazy pool initialization (#81) (#84) - ([34b0d56](https://github.com/haymon-ai/database-mcp/commit/34b0d5671bebfa56481c6b0d01932d0d5e31d475)) - [@athopen](https://github.com/athopen)
- add structured output schemas for all MCP tools (#83) - ([9ba8430](https://github.com/haymon-ai/database-mcp/commit/9ba84308782ffb048b4003d1eaa23ed402ab4539)) - [@athopen](https://github.com/athopen)
- add explain_query tool for execution plan analysis (#77) - ([6f255e0](https://github.com/haymon-ai/database-mcp/commit/6f255e0a27cbdf8ec2e95a4b120b891e9a1c3b7b)) - [@athopen](https://github.com/athopen)
- add drop_table tool (#76) - ([e505e74](https://github.com/haymon-ai/database-mcp/commit/e505e74feb38d1c1a954183e3c340a857b090826)) - [@athopen](https://github.com/athopen)
- add drop_database tool (#72) (#75) - ([c564dc8](https://github.com/haymon-ai/database-mcp/commit/c564dc8417da4250670daf139e9fb777b753aa3c)) - [@athopen](https://github.com/athopen)
- add per-query execution timeout with 30s default (#69) - ([697f70a](https://github.com/haymon-ai/database-mcp/commit/697f70ab35f9846114ebbf16850f0fbb23216802)) - [@athopen](https://github.com/athopen)
- add connection timeout and pool lifecycle defaults (#68) - ([2e44203](https://github.com/haymon-ai/database-mcp/commit/2e442034fb521424e209c3498b192dd55d328d9a)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- update yanked fastrand 2.4.0 to 2.4.1 (#79) - ([861d9ff](https://github.com/haymon-ai/database-mcp/commit/861d9ff65e833d44222643a56ecbecbe272db8a5)) - [@athopen](https://github.com/athopen)

- - -

## [v0.5.2](https://github.com/haymon-ai/database-mcp/compare/3c220fc3609b344e1c371fba68a27b6bbd0401a6..v0.5.2) - 2026-04-06
#### Features
- extend server info with full Implementation metadata (#66) - ([df4b47f](https://github.com/haymon-ai/database-mcp/commit/df4b47fa19fd537f1971173245709d25b1949935)) - [@athopen](https://github.com/athopen)
#### Refactoring
- migrate functional tests to MCP tool layer (#67) (#67) - ([5c75d8f](https://github.com/haymon-ai/database-mcp/commit/5c75d8f8164acabbd536c38ddd78ca309010e661)) - [@athopen](https://github.com/athopen)
- replace schemars description attributes with doc comments (#64) - ([398b9ae](https://github.com/haymon-ai/database-mcp/commit/398b9aeccbaadecf4cfd8224e5561d79b1fcd437)) - [@athopen](https://github.com/athopen)
- introduce SQLite-local request types without database_name (#63) - ([c7b1f2a](https://github.com/haymon-ai/database-mcp/commit/c7b1f2acbb7663e2f9ea88db6e8f4f10a7babd8b)) - [@athopen](https://github.com/athopen)
- restructure crate architecture and use rmcp tool macros (#62) - ([beb2ce9](https://github.com/haymon-ai/database-mcp/commit/beb2ce9e4b942c3ac57a9d7cf95870ec1c01e551)) - [@athopen](https://github.com/athopen)
- modularize binary crate into focused modules (#61) - ([f5b0f0f](https://github.com/haymon-ai/database-mcp/commit/f5b0f0fd5d8e50898787aebf18c728bc2811711f)) - [@athopen](https://github.com/athopen)

- - -

## [v0.5.1](https://github.com/haymon-ai/database-mcp/compare/9c5b55a15ab4dc0c17a45fdfbd21ac886334061b..v0.5.1) - 2026-04-01
#### Refactoring
- add database-mcp- prefix to all workspace crate names (#52) - ([9c5b55a](https://github.com/haymon-ai/database-mcp/commit/9c5b55a15ab4dc0c17a45fdfbd21ac886334061b)) - [@athopen](https://github.com/athopen)

- - -

## [v0.5.0](https://github.com/haymon-ai/database-mcp/compare/bb911f876d63a074c009a3050dc3f6241e9dd0a9..v0.5.0) - 2026-04-01
#### Features
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) add Docker image and CI pipeline (#51) - ([692df00](https://github.com/haymon-ai/database-mcp/commit/692df0074efdbb9a73a139dfafb8ae9ce5d87282)) - [@athopen](https://github.com/athopen)
- add glama.json for Glama MCP registry - ([9640a97](https://github.com/haymon-ai/database-mcp/commit/9640a97856de2dd15ab76a72ceec84f553973a86)) - [@athopen](https://github.com/athopen)
- add server.json for Official MCP Registry publishing - ([bb911f8](https://github.com/haymon-ai/database-mcp/commit/bb911f876d63a074c009a3050dc3f6241e9dd0a9)) - [@athopen](https://github.com/athopen)

- - -

## [v0.4.0](https://github.com/haymon-ai/database/compare/f9bfb9d2c32078499027fe0fe63fe646c5217af3..v0.4.0) - 2026-03-31
#### Features
- add security policy, Dependabot config, and audit workflow (#39) - ([1a2f8a6](https://github.com/haymon-ai/database/commit/1a2f8a65030f8537b1b8861a2e9b3bbe4423e292)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- (**docs**) correct download URLs in installation guide - ([17cbf70](https://github.com/haymon-ai/database/commit/17cbf707febeec2fb61b28e9d3333b778db85e52)) - [@athopen](https://github.com/athopen)
- use official cocogitto recipe for Cargo version bumping - ([3d6739d](https://github.com/haymon-ai/database/commit/3d6739d6b3e703ad4b5356ba60ad0c8f2d0c8823)) - [@athopen](https://github.com/athopen)
#### Refactoring
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) restructure into multi-crate workspace (#50) - ([69b3584](https://github.com/haymon-ai/database/commit/69b3584117727b9e6cc1a186931b6034fe5aab80)) - [@athopen](https://github.com/athopen)
- flatten tool dispatch by inlining forwarding layer into handlers (#49) - ([d2be107](https://github.com/haymon-ai/database/commit/d2be107a36fd8577c97c27b238b8edfdfe21abb9)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) merge get_table_schema and get_table_schema_with_relations into single tool (#48) - ([b1410be](https://github.com/haymon-ai/database/commit/b1410be4717069f01a98db0414ada12884e67085)) - [@athopen](https://github.com/athopen)

- - -

## [v0.3.1](https://github.com/haymon-ai/database/compare/47bbb336670cdc0e4542b10f3ff9c74614017644..v0.3.1) - 2026-03-29
#### Features
- add structured GitHub issue templates for docs and regressions - ([47bbb33](https://github.com/haymon-ai/database/commit/47bbb336670cdc0e4542b10f3ff9c74614017644)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- use Default + field mutation for StreamableHttpServerConfig - ([bac1804](https://github.com/haymon-ai/database/commit/bac1804ce9775646448f87ed294e6e9d21a31ecf)) - [@athopen](https://github.com/athopen)

- - -

## [v0.3.0](https://github.com/haymon-ai/database/compare/ae40099963bd50181fd76fbd0a61b207d5f4ccda..v0.3.0) - 2026-03-29
#### Features
- add MCP tool annotations (readOnlyHint, destructiveHint, etc.) (#37) - ([261bede](https://github.com/haymon-ai/database/commit/261bedef8acd413c0d62b41a59bc2ab9e1252782)) - [@athopen](https://github.com/athopen)
- include server name and version in ServerInfo (#35) (#36) - ([071b4b7](https://github.com/haymon-ai/database/commit/071b4b7f89518b2b69c63fabc70e4549ae00723e)) - [@athopen](https://github.com/athopen)
- dynamic tool registration based on backend and read-only flag (#32) - ([ae40099](https://github.com/haymon-ai/database/commit/ae40099963bd50181fd76fbd0a61b207d5f4ccda)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- use correct cocogitto authors config syntax - ([08f96a7](https://github.com/haymon-ai/database/commit/08f96a7f8cc249d7b25c5c429c83dfd327f75d1a)) - [@athopen](https://github.com/athopen)
#### Documentation
- add demo gif to README - ([b2f6b5a](https://github.com/haymon-ai/database/commit/b2f6b5a9a2b923d543d9d03d6c5eb1c238db1081)) - [@athopen](https://github.com/athopen)

- - -

## [v0.2.0](https://github.com/haymon-ai/database/compare/e00e191f7f1cfba9099d1db00469c9f365b5de6c..v0.2.0) - 2026-03-29
#### Features
- add --version flag and version subcommand (#31) - ([4d69220](https://github.com/haymon-ai/database/commit/4d69220685b98ccd296941994fd44a36386bdc82)) - Andreas Penz
#### Documentation
- add Fumadocs documentation site with GitHub Pages deployment (#29) - ([9f2f366](https://github.com/haymon-ai/database/commit/9f2f3663b92cd7e79459ff727d5d4d537528ab64)) - Andreas Penz
#### Refactoring
- rename sql-mcp to database-mcp (#28) - ([bc47f65](https://github.com/haymon-ai/database/commit/bc47f65e63e51f68de7e11f0fc7ae4198d0e25b7)) - Andreas Penz

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).