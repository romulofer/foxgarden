# FoxGarden

## Project Overview

The ultimate objective of this project is to build a full fledged, but light spring boot IDE, to work with maven, kotlin, java. But as a first checkpoint let's just build a simple code editor for .java and .kt files,

FoxGarden should be lightning fast, the way [Zed](https://zed.dev) is — that's the
performance bar, not just a nice-to-have. Zed's source is checked out at
`../references/zed` (relative to this repo) for reference: consult it when
making architecture or performance decisions (e.g. how it handles incremental
parsing, rendering, or startup cost) rather than guessing at how a
lightning-fast editor should be built. `../references/java` and
`../references/kotlin` are Zed's own Java and Kotlin extensions — the
concrete reference for how the eventual LSP/build-tooling integration
(Maven/Gradle awareness, JDTLS/Kotlin Language Server, debug adapters) that
later checkpoints call for should be approached.

## Tech Stack

| Category | Technology | Version / Notes |
|----------|------------|-----------------|
| backend | rust | latest |
| frontend | rust | use whatever libraries are necessary |


## Core Features

- simple error/deprecation signals in the form of squiggly lines
- files should open in tabs
- side panel with the open project
- changed files should contain an asterisk beside the name

## Acceptance Criteria

- tests 

## What Not To Do

- do not author commits
- do not push upstream