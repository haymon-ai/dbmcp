# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## [v0.21.0](https://github.com/haymon-ai/dbmcp/compare/2a8aa1f92f4de2a0ec45aba1012891ee7b4f5936..v0.21.0) - 2026-11-24
#### 🚀 Features
- (**plugin**) ship Claude Code and Codex plugins (#433) - ([1e2f31c](https://github.com/haymon-ai/dbmcp/commit/1e2f31c96268d61452715cdd50aa2888cbf36a10)) - [@athopen](https://github.com/athopen)
- test every supported database version in CI and warn on others - ([bcc8f47](https://github.com/haymon-ai/dbmcp/commit/bcc8f47457b17a7b58ea183eb7c838bf93787395)) - [@athopen](https://github.com/athopen)
#### 📚 Documentation
- consolidate project messaging (#432) - ([1f322ca](https://github.com/haymon-ai/dbmcp/commit/1f322cad95905031666cb2660d8fbaa91a204a7e)) - [@athopen](https://github.com/athopen)
#### 🚜 Refactoring
- (**policy**) share the column-rules enforcement envelope (#431) - ([63a3de7](https://github.com/haymon-ai/dbmcp/commit/63a3de76326f88ff908caa3102e4fe33cc8bedf7)) - [@athopen](https://github.com/athopen)

- - -

## [v0.20.0](https://github.com/haymon-ai/dbmcp/compare/b4c48ee3f88b2cc27905877bbe0b0ba353bd6b39..v0.20.0) - 2026-11-23

#### 💥 Breaking Changes
- (**privacy**) the pii section is now privacy, and DBMCP_PII_* env vars are now DBMCP_PRIVACY_*. A YAML pii: section fails as an unknown field, and a set DBMCP_PII_* variable stops startup naming its replacement. pii.enabled and pii.ner_enabled are replaced by privacy.mode (none | rules | full), which now defaults to full: set privacy.mode: none (DBMCP_PRIVACY_MODE=none) to turn redaction off. NER runs under full when privacy.model is set. Without privacy.key, hash tokens use a random per-process key. The crate dbmcp-pii is renamed dbmcp-privacy. Redaction cost (regex, full): ~0.75 s per 1000-row x 10-column page, ~0.5 s per 64 KiB text cell. - ([41ed1af](https://github.com/haymon-ai/dbmcp/commit/41ed1af3782ac218feb94be3a45896951b74b256))
- (**sql-lineage**) readQuery rejects SHOW WARNINGS, SHOW ERRORS and SHOW COUNT(*) WARNINGS/ERRORS. - ([c0025e6](https://github.com/haymon-ai/dbmcp/commit/c0025e68163e227a7d924d9e0efd1b58caad58c7))
#### 🚀 Features
- (**mysql**) return MySQL 9 VECTOR values as number arrays (#377) - ([dbf0a6c](https://github.com/haymon-ai/dbmcp/commit/dbf0a6c5cf2564d6fc0298fc35431c518062d566)) - [@athopen](https://github.com/athopen)
- (**mysql**) enforce pii.rules column rules (#347) - ([58bec58](https://github.com/haymon-ai/dbmcp/commit/58bec5814529664186950db9229c5e2c3d26c440)) - [@athopen](https://github.com/athopen)
- (**pii**) accept shorter pii.rules column keys (#401) - ([5dc9acf](https://github.com/haymon-ai/dbmcp/commit/5dc9acf253c0791997f6330c081306d30e1501e2)) - [@athopen](https://github.com/athopen)
- (**pii**) redact ruled columns by position (#344) - ([cc2be20](https://github.com/haymon-ai/dbmcp/commit/cc2be209a6290d36b2efb5b220e2278870c985cd)) - [@athopen](https://github.com/athopen)
- (**pii**) plan column rules from lineage (#343) - ([69b13ba](https://github.com/haymon-ai/dbmcp/commit/69b13ba5ffe3cc2f5485c86573cdda0362ac99fb)) - [@athopen](https://github.com/athopen)
- (**policy**) prune explainQuery plans of ruled relations (#371) - ([1c68560](https://github.com/haymon-ai/dbmcp/commit/1c68560b14719e3d34f901bdeab5f5fa9cfc2890)) - [@athopen](https://github.com/athopen)
- (**postgres**) enforce pii.rules column rules (#346) - ([afc64b8](https://github.com/haymon-ai/dbmcp/commit/afc64b8a977beda27599dd4d6ae9cf90b581251d)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**privacy**) privacy on by default with privacy.mode (#416) - ([41ed1af](https://github.com/haymon-ai/dbmcp/commit/41ed1af3782ac218feb94be3a45896951b74b256)) - [@athopen](https://github.com/athopen)
- (**server**) add per-call explainQuery timeout (#394) - ([88e0dc0](https://github.com/haymon-ai/dbmcp/commit/88e0dc0b7c62599504a87e0b771538583df1030c)) - [@athopen](https://github.com/athopen)
- (**sql-lineage**) expand view bodies (#340) - ([f692c25](https://github.com/haymon-ai/dbmcp/commit/f692c253026bc299fdcf7e0d0f3be2e8c9e8406a)) - [@athopen](https://github.com/athopen)
- (**sql-lineage**) bind PostgreSQL whole-row references (#339) - ([033e0f2](https://github.com/haymon-ai/dbmcp/commit/033e0f2d9ace1446a34a12c730e219fed6ca50b5)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**sql-lineage**) mark built-in functions as known (#337) - ([c0025e6](https://github.com/haymon-ai/dbmcp/commit/c0025e68163e227a7d924d9e0efd1b58caad58c7)) - [@athopen](https://github.com/athopen)
- (**sql-lineage**) resolve columns against a catalog (#335) - ([817347b](https://github.com/haymon-ai/dbmcp/commit/817347b5f2d6afaef6e406a252add56674f7f7c5)) - [@athopen](https://github.com/athopen)
- (**sqlite**) enforce pii.rules column rules (#345) - ([b0ab848](https://github.com/haymon-ai/dbmcp/commit/b0ab848b3353e55e87618c7c1fae110311333840)) - [@athopen](https://github.com/athopen)
- (**sqlx-json**) read rows as ordered entries (#334) - ([46f0e4d](https://github.com/haymon-ai/dbmcp/commit/46f0e4d4f84abe60c2969dbf5746f3e3baa4e559)) - [@athopen](https://github.com/athopen)
- keep source key order in tool output (#356) - ([3f5b7b3](https://github.com/haymon-ai/dbmcp/commit/3f5b7b320b759edb158b478605fb1a138cea9530)) - [@athopen](https://github.com/athopen)
#### 🐛 Bug Fixes
- (**deps**) bump url to 2.5.9 for RUSTSEC-2026-0592 (#417) - ([c3896df](https://github.com/haymon-ai/dbmcp/commit/c3896df88fb0082d742491d425336c887b73f014)) - [@athopen](https://github.com/athopen)
- (**mysql**) run explainQuery analyze as ANALYZE FORMAT=JSON on MariaDB (#395) - ([c71b304](https://github.com/haymon-ai/dbmcp/commit/c71b304273b60ed7af37209b8a4c2934a99c7493)) - [@athopen](https://github.com/athopen)
- (**pii**) refuse index and optimizer hints while column rules exist (#420) - ([fcafcde](https://github.com/haymon-ai/dbmcp/commit/fcafcded5a442c35f0a510510bd86224882f054b)) - [@athopen](https://github.com/athopen)
- (**pii**) hash column rules under a key global detection never uses (#418) - ([9b4303c](https://github.com/haymon-ai/dbmcp/commit/9b4303cbb03cb485f65ce93dd374090dd2cd84fa)) - [@athopen](https://github.com/athopen)
- (**pii**) stop select-list conditions on ruled columns from restoring the WHERE oracle (#412) - ([9b83240](https://github.com/haymon-ai/dbmcp/commit/9b83240bab3415013eee883744fabe5da7a2bd46)) - [@athopen](https://github.com/athopen)
- (**pii**) withhold MySQL out-of-range and too-long errors (#413) - ([c7fc0da](https://github.com/haymon-ai/dbmcp/commit/c7fc0da2723b53ae7beeda6f192fac66d64d5087)) - [@athopen](https://github.com/athopen)
- (**pii**) withhold PostgreSQL class-54 errors and refuse unknown cast types (#410) - ([68af0a8](https://github.com/haymon-ai/dbmcp/commit/68af0a8be9fc4f63fc6b088d030cdc5819c9ff46)) - [@athopen](https://github.com/athopen)
- (**pii**) refuse hash tokens of chosen, computed, or server-filled values (#404) - ([d522094](https://github.com/haymon-ai/dbmcp/commit/d5220944c31c5ab10de7fe30a956572260f4ef9e)) - [@athopen](https://github.com/athopen)
- (**pii**) skip NER spans that are listed codes or namespaced identifiers (#399) - ([d68031d](https://github.com/haymon-ai/dbmcp/commit/d68031d51058d71d6c6c348a91dc75e1e3f0fee0)) - [@athopen](https://github.com/athopen)
- (**pii**) label postcodes in generic columns as POSTCODE, not POSTCODE_DE (#397) - ([3333181](https://github.com/haymon-ai/dbmcp/commit/3333181da7fbefecce99d4e2db75f62d06a66a18)) - [@athopen](https://github.com/athopen)
- (**pii**) require context for checksum IDs that business numbers share (#396) - ([40bb18f](https://github.com/haymon-ai/dbmcp/commit/40bb18ff0c394c2edb92ac2747c0aa98ead9e93a)) - [@athopen](https://github.com/athopen)
- (**pii**) trim overlapping detections instead of dropping them (#378) - ([a0738a5](https://github.com/haymon-ai/dbmcp/commit/a0738a5c033c81137da278549aa23a067c975756)) - [@athopen](https://github.com/athopen)
- (**policy**) apply pii.rules across PostgreSQL inheritance and partitions (#385) - ([4f8c32f](https://github.com/haymon-ai/dbmcp/commit/4f8c32f7d5ae48753ea37cc03eebc2c217b37d89)) - [@athopen](https://github.com/athopen)
- (**policy**) deny relations by written name before the catalog fetch (#369) - ([9d25f1f](https://github.com/haymon-ai/dbmcp/commit/9d25f1f98944093e968e7c02356f46890002906f)) - [@athopen](https://github.com/athopen)
- (**policy**) allowlist MySQL and MariaDB SHOW forms under rules (#357) - ([8f08ccb](https://github.com/haymon-ai/dbmcp/commit/8f08ccbec60e626aa1eb6eff32fa3f77f4cf3171)) - [@athopen](https://github.com/athopen)
- (**sql**) reject MySQL comments the server reads differently from the parser (#415) - ([26039df](https://github.com/haymon-ai/dbmcp/commit/26039df022d1d8a15c1cefb91fb828260ccde724)) - [@athopen](https://github.com/athopen)
- (**sql**) reject executable comments on MySQL/MariaDB (#338) - ([6e6d853](https://github.com/haymon-ai/dbmcp/commit/6e6d853eb75dabf449fe2d9efe1e26650379b86c)) - [@athopen](https://github.com/athopen)
- (**sql-lineage**) match each server's star order over USING and NATURAL joins (#411) - ([588eb36](https://github.com/haymon-ai/dbmcp/commit/588eb36a820b13bd1110ea5690d96037207d5eee)) - [@athopen](https://github.com/athopen)
- (**sqlite**) filter COMMIT errors through value-free allowlist (#349) - ([32eb6e3](https://github.com/haymon-ai/dbmcp/commit/32eb6e36f325b347dc3fd3fd46b7af604470d8e7)) - [@athopen](https://github.com/athopen)
- refuse statements when database.backend mismatches the MySQL-family server (#370) - ([de754c1](https://github.com/haymon-ai/dbmcp/commit/de754c1f919c915d100954284a79c84aa88f6ccb)) - [@athopen](https://github.com/athopen)
- classify SHOW into a closed enum and refuse SHOW filters (#359) - ([2519a5e](https://github.com/haymon-ai/dbmcp/commit/2519a5ea3ce16de68e83a1e2098a7524ec9be16a)) - [@athopen](https://github.com/athopen)
#### 📚 Documentation
- (**mysql**) document MariaDB JSON columns returned as strings (#403) - ([ea768d6](https://github.com/haymon-ai/dbmcp/commit/ea768d6f228ab82919fd85be6830d31896cf397b)) - [@athopen](https://github.com/athopen)
- (**privacy**) document plan-chosen row order as a rank oracle (#421) - ([536e2a3](https://github.com/haymon-ai/dbmcp/commit/536e2a323e4b9efffc01a609eaa2024bf52ffb2f)) - [@athopen](https://github.com/athopen)
- (**privacy**) document column rules (#348) - ([4607f52](https://github.com/haymon-ai/dbmcp/commit/4607f52227dcbf3d2b80f16f5ddd76272537b766)) - [@athopen](https://github.com/athopen)
- (**sql**) remove stale InvalidIdentifier error docs (#390) - ([e333015](https://github.com/haymon-ai/dbmcp/commit/e3330154ed1b9cc202cc8dfb2f3cdacc4747ec9d)) - [@athopen](https://github.com/athopen)
- fix broken rustdoc intra-doc links (#393) - ([092f000](https://github.com/haymon-ai/dbmcp/commit/092f00052b7ea99e5ec1bbdda0ea1442e929b408)) - [@athopen](https://github.com/athopen)
- clarify explainQuery analyze behavior (#384) - ([b724548](https://github.com/haymon-ai/dbmcp/commit/b724548d779ab6dbf43cc0852e93a47262aaddd2)) - avaldivieso-meraki, Arian Valdivieso, athopen
#### 🚜 Refactoring
- (**policy**) decide error withholding in Planned (#355) - ([81d2a03](https://github.com/haymon-ai/dbmcp/commit/81d2a03c27b3bd85f329f1143d207ae610b81de8)) - [@athopen](https://github.com/athopen)
- (**policy**) own the result-side protocol in Planned (#354) - ([002b544](https://github.com/haymon-ai/dbmcp/commit/002b5441f617a960649f6a1b0bdefec4c6436cb4)) - [@athopen](https://github.com/athopen)
- (**policy**) plan shadowed create targets in the planner (#353) - ([cc27aed](https://github.com/haymon-ai/dbmcp/commit/cc27aed8c97172c9a23d174067d904a3ae91393b)) - [@athopen](https://github.com/athopen)
- (**policy**) move column-rules planner into dbmcp-policy (#352) - ([45b34ec](https://github.com/haymon-ai/dbmcp/commit/45b34ecdb8c5d49807889733ce65c57388c804ff)) - [@athopen](https://github.com/athopen)
- (**server**) drop method_not_found overrides now that rmcp 3.11 answers -32601 (#376) - ([88cf2e4](https://github.com/haymon-ai/dbmcp/commit/88cf2e41cba84b352cdce7d74b416bfe0e5c377f)) - [@athopen](https://github.com/athopen)
- extract truncate into dbmcp-truncate crate (#360) - ([4f2ba58](https://github.com/haymon-ai/dbmcp/commit/4f2ba587fb7893ceb12bc68b6d67dd3da9cd26e3)) - [@athopen](https://github.com/athopen)

- - -

## [v0.19.0](https://github.com/haymon-ai/dbmcp/compare/fb98def0fae51b8984215fac50cf7e7746bac488..v0.19.0) - 2026-11-16

#### ⚠ BREAKING CHANGES
- (**pii**) identifiable keyed-hash placeholders (#327) - ([68cf0bd](https://github.com/haymon-ai/dbmcp/commit/68cf0bdad816b20ec66522124ad1a9dcfaff37fd))
- dbmcp-sql Connection::read_json, execute_json, and fetch_read_only take a row sink; read_json returns () and execute_json returns (rows_affected, last_insert_id). - ([db9a650](https://github.com/haymon-ai/dbmcp/commit/db9a6501b3e8fd05cffe11247ca9c82ad33bb648))
- (**sql**) with read-only mode disabled, explainQuery no longer runs EXPLAIN ANALYZE of write statements (MySQL, MariaDB, PostgreSQL), and MySQL/MariaDB reject a plain EXPLAIN of a write statement. - ([d10d660](https://github.com/haymon-ai/dbmcp/commit/d10d6601df85cc072b2b4b1f19de6b250777ee4e))
#### 🚀 Features
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) identifiable keyed-hash placeholders (#327) - ([68cf0bd](https://github.com/haymon-ai/dbmcp/commit/68cf0bdad816b20ec66522124ad1a9dcfaff37fd)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) page readQuery inside the statement's own LIMIT (#326) - ([db9a650](https://github.com/haymon-ai/dbmcp/commit/db9a6501b3e8fd05cffe11247ca9c82ad33bb648)) - [@athopen](https://github.com/athopen)
- make every pooled connection a read-only session in read-only mode (#325) - ([436150d](https://github.com/haymon-ai/dbmcp/commit/436150dd0b9585dd831bfd4e528c29fafa6720e0)) - [@athopen](https://github.com/athopen)
#### 🐛 Bug Fixes
- (**config**) drop serde-saphyr developer hints from YAML parse errors (#331) - ([129832d](https://github.com/haymon-ai/dbmcp/commit/129832d5ee2df40037c23300c029247a939bb326)) - [@athopen](https://github.com/athopen)
- (**sqlx-json**) return values that decoded to null on every backend (#328) - ([e5b3461](https://github.com/haymon-ai/dbmcp/commit/e5b3461bf4e294ce43de1bc0c53ffd9ebebab216)) - [@athopen](https://github.com/athopen)
#### 📚 Documentation
- improve SEO and LLM readiness of the docs site (#321) - ([364b8bf](https://github.com/haymon-ai/dbmcp/commit/364b8bfe86fda0678f25dade3458402658cd4d4d)) - [@athopen](https://github.com/athopen)
- redesign the docs site and split it into small focused pages (#320) - ([fb98def](https://github.com/haymon-ai/dbmcp/commit/fb98def0fae51b8984215fac50cf7e7746bac488)) - [@athopen](https://github.com/athopen)
#### 🚜 Refactoring
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**sql**) move read-only enforcement behind the Connection seam (#322) - ([d10d660](https://github.com/haymon-ai/dbmcp/commit/d10d6601df85cc072b2b4b1f19de6b250777ee4e)) - [@athopen](https://github.com/athopen)

- - -

## [v0.18.1](https://github.com/haymon-ai/dbmcp/compare/0c18eb5d7426d13fe0f6642e18b918a43baa8de8..v0.18.1) - 2026-11-11
#### 🐛 Bug Fixes
- (**release**) pin cosign-installer to an existing tag (#319) - ([0c18eb5](https://github.com/haymon-ai/dbmcp/commit/0c18eb5d7426d13fe0f6642e18b918a43baa8de8)) - [@athopen](https://github.com/athopen)

- - -

## [v0.18.0](https://github.com/haymon-ai/dbmcp/compare/1290287c73a1c30a9389178549ca8d900f05d284..v0.18.0) - 2026-11-11
#### 🚀 Features
- (**docker**) build image from source on distroless debian13 (#293) - ([3128e75](https://github.com/haymon-ai/dbmcp/commit/3128e75bdf52db4af21e89fd569157d8bf3df239)) - [@athopen](https://github.com/athopen)
- (**helm**) publish a Helm chart as OCI to GHCR (#291) - ([b71294e](https://github.com/haymon-ai/dbmcp/commit/b71294e9948771ce7df2b0bf3136c27dd4d4d3c8)) - [@athopen](https://github.com/athopen)
- (**http**) add /healthz and /readyz health probes (#289) - ([71291da](https://github.com/haymon-ai/dbmcp/commit/71291da2535eb2f450e1c63bc5f7d2ab59d3cd8e)) - [@athopen](https://github.com/athopen)
- (**release**) list the crates.io package in the MCP Registry (#288) - ([ee36c58](https://github.com/haymon-ai/dbmcp/commit/ee36c580cc09bf8cf5379c0de1c2b979a5a2628a)) - [@athopen](https://github.com/athopen)
- (**release**) publish checksums, attestations and image signatures (#287) - ([ee81918](https://github.com/haymon-ai/dbmcp/commit/ee819183954c1f6d727a98bf2ddf22ed613e0379)) - [@athopen](https://github.com/athopen)
- (**server**) add per-call readQuery timeout and explainQuery hint (#316) - ([b57b792](https://github.com/haymon-ai/dbmcp/commit/b57b792fcf79e9716e7c95b16fde24b0272df3bb)) - [@athopen](https://github.com/athopen)
- (**server**) cap query output per cell and per response (#314) - ([b19eef8](https://github.com/haymon-ai/dbmcp/commit/b19eef89924d67a9449ef50074d5fb8c5622abf7)) - [@athopen](https://github.com/athopen)
- (**server**) mark database content as untrusted data (#312) - ([8a9df3b](https://github.com/haymon-ai/dbmcp/commit/8a9df3bd24c6c9ed63c83b3f6ff69cee300271b4)) - [@athopen](https://github.com/athopen)
- (**server**) return rowsAffected and lastInsertId from writeQuery (#311) - ([a206404](https://github.com/haymon-ai/dbmcp/commit/a206404df9a657cd40676bc96b9990b6845c700c)) - [@athopen](https://github.com/athopen)
#### 🐛 Bug Fixes
- (**release**) wait for npm before publishing to the MCP Registry (#286) - ([1290287](https://github.com/haymon-ai/dbmcp/commit/1290287c73a1c30a9389178549ca8d900f05d284)) - [@athopen](https://github.com/athopen)
- (**server**) clear MCP Inspector schema and definition findings (#292) - ([3e7cbd5](https://github.com/haymon-ai/dbmcp/commit/3e7cbd5d6052e706ec37a450a073b9e0610e373b)) - [@athopen](https://github.com/athopen)
- (**sql**) reject USE in readQuery and writeQuery (#309) - ([2fc61d0](https://github.com/haymon-ai/dbmcp/commit/2fc61d0abfc9014658ddb08f7a6e2215fe476f22)) - [@athopen](https://github.com/athopen)
- (**sql**) cancel timed-out queries on the database server (#308) - ([1d5accc](https://github.com/haymon-ai/dbmcp/commit/1d5acccdc9e2df12574abb2d16a7b02d116ced03)) - [@athopen](https://github.com/athopen)
- (**sqlite**) interrupt statements that exceed the query timeout (#315) - ([16330a3](https://github.com/haymon-ai/dbmcp/commit/16330a38fa4ec9bc3d5ad0b06f1d285eda88e380)) - [@athopen](https://github.com/athopen)
- (**sqlite**) open database read-only in read-only mode (#310) - ([fc6102e](https://github.com/haymon-ai/dbmcp/commit/fc6102e242a9350bd23d1c0ee6948f87d80f4dc6)) - [@athopen](https://github.com/athopen)
#### 📚 Documentation
- (**clients**) add one-click install buttons and per-client setup (#290) - ([0d0cceb](https://github.com/haymon-ai/dbmcp/commit/0d0cceb294b1fe26e69b59432b79c12930bc992e)) - [@athopen](https://github.com/athopen)
- (**pii**) list all 46 regex and 5 NER entities (#313) - ([e265331](https://github.com/haymon-ai/dbmcp/commit/e2653319aa19c7541680f49225fbcec41dbf2923)) - [@athopen](https://github.com/athopen)
- (**security**) document parse-level multi-statement blocking (#318) - ([ec36e25](https://github.com/haymon-ai/dbmcp/commit/ec36e253019863cb5ea5d665e4670cb4f06435ae)) - [@athopen](https://github.com/athopen)

- - -

## [v0.17.0](https://github.com/haymon-ai/dbmcp/compare/3cf71276757e3c41e59971487cb898134ed075de..v0.17.0) - 2026-11-07
#### 🚀 Features
- (**release**) publish npm launcher for npx installs (#285) - ([3cf7127](https://github.com/haymon-ai/dbmcp/commit/3cf71276757e3c41e59971487cb898134ed075de)) - [@athopen](https://github.com/athopen)

- - -

## [v0.16.0](https://github.com/haymon-ai/dbmcp/compare/fc178d0d0c9da52c9eb78382f34a2ca1592c8f51..v0.16.0) - 2026-11-04

#### ⚠ BREAKING CHANGES
- (**release**) no prebuilt binary for Intel macOS (x86_64-apple-darwin) is published anymore, and install.sh rejects Intel Macs. Use the Docker image instead. - ([850448f](https://github.com/haymon-ai/dbmcp/commit/850448f487d7b684d7ae732c1a5c6af829c984f9))
- (**config**) relative paths in env files no longer resolve against the working directory. - ([d95d067](https://github.com/haymon-ai/dbmcp/commit/d95d067ac8a0dd890bb91003aefebeff4db72c2a))
- (**config**) --config, --env and DBMCP_CONFIG are replaced by --config-file, --env-file and DBMCP_CONFIG_FILE. The public LoadOptions.config field is renamed config_files. - ([76de600](https://github.com/haymon-ai/dbmcp/commit/76de600dcba4c58adf3201f457d70c081c390fce))
- (**config**) every dbmcp-config module is private. ConfigError, Diagnostic, Source, the env setting table, expand_tilde, the section validate() methods, and LoadError::diagnostics() are no longer public. ssh::home_dir moves to the crate root as dbmcp_config::home_dir. - ([5d09a37](https://github.com/haymon-ai/dbmcp/commit/5d09a37ed3436c34a547c8946294d17ae63cd5e5))
- (**security**) in read-only mode, explainQuery on MySQL and MariaDB rejects plain EXPLAIN of INSERT, UPDATE and DELETE statements. - ([14fe562](https://github.com/haymon-ai/dbmcp/commit/14fe5627d0e2f50094013fa1f2062983a194a523))
- (**cli**) every per-setting CLI flag is removed, including --transport and --log-level; passing one is now a usage error. Use the matching DBMCP_* env var, the --env file, or the --config YAML file instead (see the migration table in the configuration docs). Boolean values now accept only 1/0/t/f/true/false (and TRUE/True/FALSE/False); yes/no/on/off are rejected. - ([dbf716d](https://github.com/haymon-ai/dbmcp/commit/dbf716dfb93568b580049a1e6034dd9ddbaf46d6))
- (**cli**) dbmcp_config::Secret and the secret module are removed; config fields holding credentials are Option<secrecy::SecretString>. - ([dbf716d](https://github.com/haymon-ai/dbmcp/commit/dbf716dfb93568b580049a1e6034dd9ddbaf46d6))
- (**cli**) the dbmcp-config enums (Transport, LogLevel, DatabaseBackend, PiiOperator, PiiCategory, StrictHostKeyChecking) no longer implement clap::ValueEnum. - ([dbf716d](https://github.com/haymon-ai/dbmcp/commit/dbf716dfb93568b580049a1e6034dd9ddbaf46d6))
- (**cli**) the stdio, http and version subcommands are removed. `dbmcp stdio ...` becomes `dbmcp ...`, `dbmcp http ...` becomes `dbmcp --transport http ...`, `dbmcp version` becomes `dbmcp --version`. MCP client configs drop `"args": ["stdio"]`. The Docker image no longer sets CMD; pass `--transport http` for HTTP. - ([fc178d0](https://github.com/haymon-ai/dbmcp/commit/fc178d0d0c9da52c9eb78382f34a2ca1592c8f51))
- (**cli**) every environment variable gains a DBMCP_ prefix (DB_HOST -> DBMCP_DB_HOST, PII_ENABLE -> DBMCP_PII_ENABLE, ...). Old names are ignored without warning: an unrenamed PII_ENABLE silently disables PII redaction, an unrenamed SSH_HOST silently connects without the tunnel, an unrenamed DB_READ_ONLY falls back to read-only. Rename .env files with: - ([fc178d0](https://github.com/haymon-ai/dbmcp/commit/fc178d0d0c9da52c9eb78382f34a2ca1592c8f51))
- (**cli**) HTTP options are ignored under stdio, but an unparsable value (e.g. DBMCP_HTTP_PORT=abc) now fails startup in every transport. - ([fc178d0](https://github.com/haymon-ai/dbmcp/commit/fc178d0d0c9da52c9eb78382f34a2ca1592c8f51))
#### 🚀 Features
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**cli**) load settings from env, env file, and YAML config (#264) - ([dbf716d](https://github.com/haymon-ai/dbmcp/commit/dbf716dfb93568b580049a1e6034dd9ddbaf46d6)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**cli**) replace transport subcommands with --transport flag and DBMCP_ env prefix (#263) - ([fc178d0](https://github.com/haymon-ai/dbmcp/commit/fc178d0d0c9da52c9eb78382f34a2ca1592c8f51)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**config**) resolve env-file paths against their file and expand ~ in config paths (#271) - ([d95d067](https://github.com/haymon-ai/dbmcp/commit/d95d067ac8a0dd890bb91003aefebeff4db72c2a)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**config**) load multiple config and env files (#270) - ([76de600](https://github.com/haymon-ai/dbmcp/commit/76de600dcba4c58adf3201f457d70c081c390fce)) - [@athopen](https://github.com/athopen)
- (**config**) validate dbmcp.yaml against an embedded JSON Schema (#267) - ([8c64651](https://github.com/haymon-ai/dbmcp/commit/8c6465188609eeb4d6b0f4f5bb362b15334072ea)) - [@athopen](https://github.com/athopen)
#### 🐛 Bug Fixes
- (**deps**) pin ort to 2.0.0-rc.10 to fix macOS release build - ([4af272c](https://github.com/haymon-ai/dbmcp/commit/4af272cc0df0dd55ec03693d75b2c35be4c34f6e)) - athopen
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**release**) drop Intel macOS release binary - ([850448f](https://github.com/haymon-ai/dbmcp/commit/850448f487d7b684d7ae732c1a5c6af829c984f9)) - athopen
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**security**) enforce read-only mode with read-only transactions - ([14fe562](https://github.com/haymon-ai/dbmcp/commit/14fe5627d0e2f50094013fa1f2062983a194a523)) - athopen
#### 🚜 Refactoring
- (**cli**) own tunnel and server lifetime in server::serve (#265) - ([08c8461](https://github.com/haymon-ai/dbmcp/commit/08c846174feb58de2e621c2a184e2a9e87db0f43)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**config**) validate through Config and shrink public interface (#266) - ([5d09a37](https://github.com/haymon-ai/dbmcp/commit/5d09a37ed3436c34a547c8946294d17ae63cd5e5)) - [@athopen](https://github.com/athopen)

- - -

## [v0.15.2](https://github.com/haymon-ai/dbmcp/compare/e17717a3059b4ec33ecf78e7abe0d36938ad0785..v0.15.2) - 2026-10-26
#### Bug Fixes
- (**deps**) bump crypto-bigint and ctutils for RUSTSEC advisories (#262) - ([e17717a](https://github.com/haymon-ai/dbmcp/commit/e17717a3059b4ec33ecf78e7abe0d36938ad0785)) - [@athopen](https://github.com/athopen)

- - -

## [v0.15.1](https://github.com/haymon-ai/dbmcp/compare/3a643969991d5f8fbe4600b1e191640990d23199..v0.15.1) - 2026-10-22
#### Bug Fixes
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**cli**) harden CLI secret handling and boolean flags (#242) - ([3a64396](https://github.com/haymon-ai/dbmcp/commit/3a643969991d5f8fbe4600b1e191640990d23199)) - [@athopen](https://github.com/athopen)
- (**deps**) bump rustls-webpki and webpki roots for emSign advisories (#258) - ([6db2f00](https://github.com/haymon-ai/dbmcp/commit/6db2f0001656dfc7829852e4e87efd8cfcaca382)) - [@athopen](https://github.com/athopen)
#### Refactoring
- (**cli**) extract CLI into dbmcp-cli crate (#244) - ([f5beb94](https://github.com/haymon-ai/dbmcp/commit/f5beb94f14178a6e047e22bc4ecb2e8f68745d24)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**server**) deepen pagination into a listing module (#245) - ([c421468](https://github.com/haymon-ai/dbmcp/commit/c42146821ad86abbe722eb665547d5814a1d6c7d)) - [@athopen](https://github.com/athopen)
- (**sqlx-json**) decode rows into cells behind one JSON encoder (#246) - ([a1ccacb](https://github.com/haymon-ai/dbmcp/commit/a1ccacb271a5df613d691d00959d7f9bbf8e08a0)) - [@athopen](https://github.com/athopen)

- - -

## [v0.15.0](https://github.com/haymon-ai/dbmcp/compare/1413a41701a4aa2324ca6d01d5c16b73c8072876..v0.15.0) - 2026-10-17
#### Features
- (**server**) align with rmcp 3.5 and return tool errors in-band (#215) - ([1413a41](https://github.com/haymon-ai/dbmcp/commit/1413a41701a4aa2324ca6d01d5c16b73c8072876)) - [@athopen](https://github.com/athopen)
- (**ssh**) reconnect lost sessions, keep alive and surface tunnel errors (#237) - ([4e33704](https://github.com/haymon-ai/dbmcp/commit/4e3370440482f40bbedd6448d857cbe12e2e3811)) - [@athopen](https://github.com/athopen)
- (**ssh**) verify host keys against known_hosts (#236) - ([7647657](https://github.com/haymon-ai/dbmcp/commit/76476576cda439de61b3ffd0efeb09edf60a27c6)) - [@athopen](https://github.com/athopen)
- (**ssh**) authenticate with private keys (#235) - ([eaf3170](https://github.com/haymon-ai/dbmcp/commit/eaf3170ece0efc54e04ec68e8d109fb595b58a99)) - [@athopen](https://github.com/athopen)
- (**ssh**) tunnel database connections through SSH with password auth (#234) - ([57d78fd](https://github.com/haymon-ai/dbmcp/commit/57d78fdbb9ab3a2a79768058ac148bb9eb6753fb)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- (**deps**) bump yanked inout 0.2.3 to 0.2.4 (#241) - ([9404699](https://github.com/haymon-ai/dbmcp/commit/94046999f95f47c6ecac0a88f3628f3faf43eb85)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**http**) match allowed origins per port in CORS and rmcp (#217) - ([953d0e4](https://github.com/haymon-ai/dbmcp/commit/953d0e4935a1bbf85a989101387dc4c0cfcea7cc)) - [@athopen](https://github.com/athopen)
- (**tests**) probe DB readiness over TCP (#216) - ([a59829a](https://github.com/haymon-ai/dbmcp/commit/a59829a406605d9b942625afff1a188ca4c837b7)) - [@athopen](https://github.com/athopen)
#### Documentation
- (**ssh**) document SSH tunnel support (#238) - ([53a531e](https://github.com/haymon-ai/dbmcp/commit/53a531ee13764991920142f9b3097a726bfc020e)) - [@athopen](https://github.com/athopen)
#### Refactoring
- (**ssh**) drop DB_ prefix from SSH env vars (#240) - ([ba26e67](https://github.com/haymon-ai/dbmcp/commit/ba26e6753139ee07fbbccede5cf1e9e83edc3a45)) - [@athopen](https://github.com/athopen)

- - -

## [v0.14.0](https://github.com/haymon-ai/dbmcp/compare/29f4efa5955c10be6e9b86ab5452d86d133ddb7a..v0.14.0) - 2026-10-15
#### Features
- (**server**) drain connection pools on graceful shutdown (#192) - ([866dd9b](https://github.com/haymon-ai/dbmcp/commit/866dd9bf957599c0214e291eaa6ff6ae105aaf82)) - [@athopen](https://github.com/athopen)
- (**sql-lineage**) add column-level SQL lineage extraction crate (#193) - ([52bf45c](https://github.com/haymon-ai/dbmcp/commit/52bf45c5cd8700004b565258f6e39735c20c9d8a)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- (**deps**) pin ort to 2.0.0-rc.10 to fix macOS release build - ([94269d5](https://github.com/haymon-ai/dbmcp/commit/94269d5d44ff23e0a632ecc6eb5f12b1288977eb)) - athopen
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**security**) require bearer token for non-loopback HTTP binds - ([3d620f3](https://github.com/haymon-ai/dbmcp/commit/3d620f316c8b7d56a0806ac7508e2010de5f18b2)) - athopen
- (**security**) extend dangerous function deny list - ([2ffc5bc](https://github.com/haymon-ai/dbmcp/commit/2ffc5bc173e219c941ff874b5e205d093b1fc95d)) - athopen
- (**security**) validate statement inside EXPLAIN in readQuery - ([f25da70](https://github.com/haymon-ai/dbmcp/commit/f25da70a7a63d52efe8fff06d075efd09761ee43)) - athopen
- (**security**) enforce single statement in explainQuery - ([eca36f5](https://github.com/haymon-ai/dbmcp/commit/eca36f51dbe7d6b47547136031fdd749f9664307)) - [@athopen](https://github.com/athopen)

- - -

## [v0.13.2](https://github.com/haymon-ai/dbmcp/compare/6fd85824148d5ca4040e838ab52092a9bd820cdb..v0.13.2) - 2026-05-29
#### Refactoring
- (**server**) unify pinned/unpinned request structs (#190) - ([15270df](https://github.com/haymon-ai/dbmcp/commit/15270dfcd9cdc21811fa7854b54e11d9fb23fcba)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**server**) unify list-tool responses into ListEntriesResponse (#189) - ([c3b24c9](https://github.com/haymon-ai/dbmcp/commit/c3b24c98718d54b39788e834047a6d41405b7ab2)) - [@athopen](https://github.com/athopen)
- (**server**) generate slim tool schemas via schemars Transform (#188) - ([5bbb631](https://github.com/haymon-ai/dbmcp/commit/5bbb631fefe8d79d729b7f49a98c5ebac60f74f8)) - [@athopen](https://github.com/athopen)
- centralize shared imports into prelude modules (#186) - ([6fd8582](https://github.com/haymon-ai/dbmcp/commit/6fd85824148d5ca4040e838ab52092a9bd820cdb)) - [@athopen](https://github.com/athopen)

- - -

## [v0.13.1](https://github.com/haymon-ai/dbmcp/compare/b9b095dcb6f6fe1632e324fb1e14ed4ed44adcb2..v0.13.1) - 2026-05-25
#### Continuous Integration
- (**release**) build aarch64-linux natively and pin runner images - ([b9b095d](https://github.com/haymon-ai/dbmcp/commit/b9b095dcb6f6fe1632e324fb1e14ed4ed44adcb2)) - athopen

- - -

## [v0.13.0](https://github.com/haymon-ai/dbmcp/compare/358a25152a3f07bd0ea2f2fdb37cbdc780fb9793..v0.13.0) - 2026-05-25
#### Features
- (**pii**) swap candle NER backend for ONNX Runtime (#179) - ([c4ec38d](https://github.com/haymon-ai/dbmcp/commit/c4ec38d97168c73cad79d04d02ad570eb674fcc3)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) NER organization, NRP & facility entities (#177) - ([965dc89](https://github.com/haymon-ai/dbmcp/commit/965dc89dfab24553eeab3b35ad8a927eb31dbb61)) - [@athopen](https://github.com/athopen)
- (**pii**) optional ML/NER PII pass (person & location) (#174) - ([358a251](https://github.com/haymon-ai/dbmcp/commit/358a25152a3f07bd0ea2f2fdb37cbdc780fb9793)) - [@athopen](https://github.com/athopen)
#### Performance Improvements
- (**pii**) batch NER inference and unify query redaction offload (#185) - ([6dbabc4](https://github.com/haymon-ai/dbmcp/commit/6dbabc45f51132fafac269133124036a9234bfb2)) - [@athopen](https://github.com/athopen)
#### Documentation
- (**pii**) improve NER model download instructions (#183) - ([fcae670](https://github.com/haymon-ai/dbmcp/commit/fcae6709ee74539320be28d426130e935b809024)) - [@athopen](https://github.com/athopen)
- (**pii**) add local NER model setup guide (#180) - ([e72439c](https://github.com/haymon-ai/dbmcp/commit/e72439cf2f8f98be4132283c3c93883f2cc7ddcf)) - [@athopen](https://github.com/athopen)

- - -

## [v0.12.1](https://github.com/haymon-ai/dbmcp/compare/2ab63dbcb935a57a10c1fc12fd2b25710d591eda..v0.12.1) - 2026-05-22
#### Features
- (**pii**) add password-hash recognizer (bcrypt, sha-crypt, argon2) (#172) - ([a3f2607](https://github.com/haymon-ai/dbmcp/commit/a3f2607ba2214f15e89523b0bcee33daf0c2df0a)) - [@athopen](https://github.com/athopen)
- (**pii**) add context-gated date-of-birth recognizer (#169) - ([9f0d5da](https://github.com/haymon-ai/dbmcp/commit/9f0d5dad362343651919c919dc8aee9902525454)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- (**pii**) redact German postcodes in English-named columns (#171) - ([a49e7c5](https://github.com/haymon-ai/dbmcp/commit/a49e7c5a53ac251d9152db0fe9ddcc62508133b2)) - [@athopen](https://github.com/athopen)
- (**pii**) scope phone detection to international-prefixed numbers (#170) - ([3643aef](https://github.com/haymon-ai/dbmcp/commit/3643aef49eb0e17a41c7c4fddbdd39efb372a026)) - [@athopen](https://github.com/athopen)
- (**pii**) context-gate weak numeric recognizers (#168) - ([2ab63db](https://github.com/haymon-ai/dbmcp/commit/2ab63dbcb935a57a10c1fc12fd2b25710d591eda)) - [@athopen](https://github.com/athopen)

- - -

## [v0.12.0](https://github.com/haymon-ai/dbmcp/compare/aadf0e60353b4df1be4de80f3ae43ae459268642..v0.12.0) - 2026-05-20
#### Features
- (**tools**) split per-database tools into Pinned/Unpinned variants (#167) - ([41b5bfa](https://github.com/haymon-ai/dbmcp/commit/41b5bfadb111ddca00218d8188860822e34e778c)) - [@athopen](https://github.com/athopen)
- (**tools**) hide cross-database tools when a database name is pinned (#165) - ([aadf0e6](https://github.com/haymon-ai/dbmcp/commit/aadf0e60353b4df1be4de80f3ae43ae459268642)) - [@athopen](https://github.com/athopen)

- - -

## [v0.11.1](https://github.com/haymon-ai/dbmcp/compare/298b65533c418f93736fdc53d8d139b64ceba234..v0.11.1) - 2026-05-17
#### Features
- (**pii**) context-aware boost + redactor walker simplifications (#162) - ([298b655](https://github.com/haymon-ai/dbmcp/commit/298b65533c418f93736fdc53d8d139b64ceba234)) - [@athopen](https://github.com/athopen)
- externalize tool descriptions + read-only-aware instructions (#164) - ([27f6b81](https://github.com/haymon-ai/dbmcp/commit/27f6b81e169cc9229f3a7ed79cb29db05e037368)) - [@athopen](https://github.com/athopen)

- - -

## [v0.11.0](https://github.com/haymon-ai/dbmcp/compare/3a93a73bb537c7935dda065298d8d067b33261e7..v0.11.0) - 2026-05-11
#### Features
- (**pii**) add 12 German recognizers (#159) - ([b9365ce](https://github.com/haymon-ai/dbmcp/commit/b9365ce0bb7283dcca016bf3a44371f61313bf02)) - [@athopen](https://github.com/athopen)
- (**pii**) add 3 UK recognizers (#158) - ([324d0ea](https://github.com/haymon-ai/dbmcp/commit/324d0eae988e84afbafef2434c6aed41e4be700d)) - [@athopen](https://github.com/athopen)
- (**pii**) add 5 US recognizers (#157) - ([3a93a73](https://github.com/haymon-ai/dbmcp/commit/3a93a73bb537c7935dda065298d8d067b33261e7)) - [@athopen](https://github.com/athopen)
#### Documentation
- (**crates**) add per-crate READMEs for crates.io (#161) - ([d3b30d6](https://github.com/haymon-ai/dbmcp/commit/d3b30d6129888f95ae1c50e2e8e7472a0d7c1a10)) - [@athopen](https://github.com/athopen)
#### Refactoring
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) rename recognizer folders + suffixes to ISO 3166-1 alpha-3 (#160) - ([235f25a](https://github.com/haymon-ai/dbmcp/commit/235f25a74a2381713d49fbb767f512c4890d955d)) - [@athopen](https://github.com/athopen)

- - -

## [v0.10.4](https://github.com/haymon-ai/dbmcp/compare/63c6f488f8fd6e8dd28706e0489a614a9e0d0f66..v0.10.4) - 2026-05-11
#### Features
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) fancy-regex + crypto checksums + recognizer reorg (#156) - ([4ab5b1f](https://github.com/haymon-ai/dbmcp/commit/4ab5b1f970bc087196833caa64c8be83de99c519)) - [@athopen](https://github.com/athopen)
#### Refactoring
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) restructure module layout and Entity enum (#155) - ([63c6f48](https://github.com/haymon-ai/dbmcp/commit/63c6f488f8fd6e8dd28706e0489a614a9e0d0f66)) - [@athopen](https://github.com/athopen)

- - -

## [v0.10.3](https://github.com/haymon-ai/dbmcp/compare/e2b54c2abcf8b770e3fd259f8be282a10d2a5ac5..v0.10.3) - 2026-05-08
#### Bug Fixes
- (**pii**) gate SIN_CA on keyword context (#150) (#154) - ([f47bdba](https://github.com/haymon-ai/dbmcp/commit/f47bdbabf31698b0ae27983b52d9f168afd10570)) - [@athopen](https://github.com/athopen)
- (**pii**) reject IPv6 false positives on Postgres ::cast operators (#153) - ([e907021](https://github.com/haymon-ai/dbmcp/commit/e90702129fab352db47d5307440dca7324b91772)) - [@athopen](https://github.com/athopen)
- (**pii**) reject VAT false positives on uppercase English words (#152) - ([3b72745](https://github.com/haymon-ai/dbmcp/commit/3b72745ca403c3b57022412d8617738b8ba64b15)) - [@athopen](https://github.com/athopen)
- (**pii**) reject leading-zero phone false positives (#147) (#151) - ([e2b54c2](https://github.com/haymon-ai/dbmcp/commit/e2b54c2abcf8b770e3fd259f8be282a10d2a5ac5)) - [@athopen](https://github.com/athopen)

- - -

## [v0.10.2](https://github.com/haymon-ai/dbmcp/compare/c090cbc7dcddaa7a1bcfe283316a82c55dd35ae8..v0.10.2) - 2026-05-07
#### Features
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) expand entity catalog + collapse traits to enums (#145) - ([f73cbd5](https://github.com/haymon-ai/dbmcp/commit/f73cbd5ead8bcce80c8660bd8712c24a86af077a)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) rename recognizer module + wire category routing through CLI (#144) - ([c090cbc](https://github.com/haymon-ai/dbmcp/commit/c090cbc7dcddaa7a1bcfe283316a82c55dd35ae8)) - [@athopen](https://github.com/athopen)

- - -

## [v0.10.1](https://github.com/haymon-ai/dbmcp/compare/04eb76107b79d16be34c6e25feb43e09804af470..v0.10.1) - 2026-05-04
#### Features
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) walk nested json/jsonb leaves during redaction (#142) - ([04eb761](https://github.com/haymon-ai/dbmcp/commit/04eb76107b79d16be34c6e25feb43e09804af470)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- (**sqlx-json**) numeric precision + null serialization (#141) (#143) - ([fbee0b6](https://github.com/haymon-ai/dbmcp/commit/fbee0b67320732a0175e32d77c7eb30fd1e30bb2)) - [@athopen](https://github.com/athopen)

- - -

## [v0.10.0](https://github.com/haymon-ai/dbmcp/compare/70372acfdf79cda250594f7448476ca22bae3fbd..v0.10.0) - 2026-05-03
#### Features
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**http**) enforce Origin header server-side via rmcp 1.6.0 (#137) - ([1b93f2f](https://github.com/haymon-ai/dbmcp/commit/1b93f2f43b70f70980db5d0705217f89134da86a)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**pii**) dedicated PII config section with operator selection (#139) - ([247e4b1](https://github.com/haymon-ai/dbmcp/commit/247e4b1a483d62fcf30446c2073a2ffec74263fd)) - [@athopen](https://github.com/athopen)
- (**pii**) regex-based PII redaction crate with opt-in query output filter (#138) - ([8fed895](https://github.com/haymon-ai/dbmcp/commit/8fed89593ee5f2f5990ee4faf79e2eceab6166ac)) - [@athopen](https://github.com/athopen)
#### Documentation
- (**pii**) document PII redaction across user-facing surfaces (#140) - ([7d924e6](https://github.com/haymon-ai/dbmcp/commit/7d924e646b6696eb2a4113f155d75c65a4585e1d)) - [@athopen](https://github.com/athopen)

- - -

## [v0.9.1](https://github.com/haymon-ai/dbmcp/compare/38acda7cc765a844607dbc81fe17288c42954351..v0.9.1) - 2026-04-29
#### Bug Fixes
- (**http**) wire --allowed-hosts into rmcp config (#133) - ([38acda7](https://github.com/haymon-ai/dbmcp/commit/38acda7cc765a844607dbc81fe17288c42954351)) - [@athopen](https://github.com/athopen)
#### Refactoring
- (**sanitize**) replace ident validation with quote-only path (#134) - ([838ce68](https://github.com/haymon-ai/dbmcp/commit/838ce68549a6eb0ac71fc03c71c0407296141bb7)) - [@athopen](https://github.com/athopen)

- - -

## [v0.9.0](https://github.com/haymon-ai/dbmcp/compare/97aac8b4a3356bba8bdaa87d17417ec70660016a..v0.9.0) - 2026-04-29
#### Features
- (**list-functions**) search + detailed mode (Postgres, MySQL, MariaDB) (#128) - ([0c1d3d4](https://github.com/haymon-ai/dbmcp/commit/0c1d3d4dfda74ccd7087a411690d71f64497b5a0)) - [@athopen](https://github.com/athopen)
- (**list-matviews**) search + detailed mode (Postgres) (#131) - ([3e44ea5](https://github.com/haymon-ai/dbmcp/commit/3e44ea565316389e058f866f4d5c49a35e063365)) - [@athopen](https://github.com/athopen)
- (**list-procedures**) search + detailed mode (Postgres, MySQL, MariaDB) (#129) - ([0e51fdc](https://github.com/haymon-ai/dbmcp/commit/0e51fdce8bb3c24520b586f11650e003f6654351)) - [@athopen](https://github.com/athopen)
- (**list-triggers**) add search + detailed mode across backends (#127) - ([7389d77](https://github.com/haymon-ai/dbmcp/commit/7389d777c1d2c5525e2e0730f6885678852ee985)) - [@athopen](https://github.com/athopen)
- (**list-views**) search + detailed mode across backends (#130) - ([6dd4b47](https://github.com/haymon-ai/dbmcp/commit/6dd4b4708d9e81eb9dbcb21c00552f9911c8ebd0)) - [@athopen](https://github.com/athopen)
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) enrich listTables with search/detailed, drop getTableSchema (#126) - ([97aac8b](https://github.com/haymon-ai/dbmcp/commit/97aac8b4a3356bba8bdaa87d17417ec70660016a)) - [@athopen](https://github.com/athopen)
#### Documentation
- add missing schema-discovery tools, polish README - ([d94278a](https://github.com/haymon-ai/dbmcp/commit/d94278a410b1c8bb5edc37cb18a29579419d0473)) - athopen

- - -

## [v0.8.0](https://github.com/haymon-ai/dbmcp/compare/52f9bad70438aa19032118b2a8b0368cd62f44a2..v0.8.0) - 2026-04-23
#### Features
- (**tools**) default `database` to configured --db-name when omitted (#121) - ([1492f28](https://github.com/haymon-ai/dbmcp/commit/1492f28d718e5780cad576b0ef910a53e3839d5c)) - [@athopen](https://github.com/athopen)
- (**tools**) add schema object discovery tools - ([52f9bad](https://github.com/haymon-ai/dbmcp/commit/52f9bad70438aa19032118b2a8b0368cd62f44a2)) - athopen
#### Bug Fixes
- (**deps**) bump rustls-webpki to 0.103.13 for RUSTSEC-2026-0104 (#124) - ([d5f1e77](https://github.com/haymon-ai/dbmcp/commit/d5f1e7793d9637a42190ec4f3eba461da821af35)) - [@athopen](https://github.com/athopen)
#### Refactoring
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) rename to dbmcp (#125) - ([3cbfb2e](https://github.com/haymon-ai/dbmcp/commit/3cbfb2e830ba5fbca6247e8a64cf454a028e7bd4)) - [@athopen](https://github.com/athopen)

- - -

## [v0.7.0](https://github.com/haymon-ai/database-mcp/compare/3026a0304d1cdc444dfe6100f28138b9043d48e5..v0.7.0) - 2026-04-20
#### Features
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**tools**) cursor pagination for list_* and read_query (#118) - ([30cb5cc](https://github.com/haymon-ai/database-mcp/commit/30cb5cc5ef383df13f2503f0b05a21659fe34854)) - [@athopen](https://github.com/athopen)
#### Bug Fixes
- (**deps**) update all dependencies to latest compatible versions (#115) (#115) - ([2463d1d](https://github.com/haymon-ai/database-mcp/commit/2463d1d99fed863bb57c056f25e4e71a231edb69)) - [@athopen](https://github.com/athopen)
- (**temporal**) decode date/time columns instead of returning null (#119) - ([17a3057](https://github.com/haymon-ai/database-mcp/commit/17a30575ee645995b2ab3e25c5fa46dc227f5313)) - [@athopen](https://github.com/athopen)
#### Refactoring
- ![BREAKING](https://img.shields.io/badge/BREAKING-red) (**api**) camelCase tool names and compact property keys (#120) - ([f56e3cd](https://github.com/haymon-ai/database-mcp/commit/f56e3cd59f8d8417333be3801a615611477fe831)) - [@athopen](https://github.com/athopen)
- (**errors**) replace monolithic AppError with per-crate SqlError (#116) - ([7fcb496](https://github.com/haymon-ai/database-mcp/commit/7fcb49668d6545135c25052046cd0b95d03e4dec)) - [@athopen](https://github.com/athopen)
- (**sql**) simplify Connection trait and harden sanitize module (#113) - ([3026a03](https://github.com/haymon-ai/database-mcp/commit/3026a0304d1cdc444dfe6100f28138b9043d48e5)) - [@athopen](https://github.com/athopen)

- - -

## [v0.6.4](https://github.com/haymon-ai/database-mcp/compare/24c25c2ed2f77c798fa415fcae311681f0427da7..v0.6.4) - 2026-04-15
#### Bug Fixes
- (**deps**) bump rustls-webpki to 0.103.12 (#112) - ([24c25c2](https://github.com/haymon-ai/database-mcp/commit/24c25c2ed2f77c798fa415fcae311681f0427da7)) - [@athopen](https://github.com/athopen)

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