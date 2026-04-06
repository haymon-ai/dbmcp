# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

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