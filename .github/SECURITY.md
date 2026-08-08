# Security Policy

A Clatch app built from this template opens a user-private Unix socket and speaks
the Clatch control pipe with real permissions on the user's machine. The template's
transport (`IPC.swift`, `ControlPipe.swift`) is the part where security matters.

## Reporting a vulnerability

**Do not open a public issue.** Use GitHub's private vulnerability reporting:
[Security → Report a vulnerability](https://github.com/arfium/clapp-template/security/advisories/new).

Expect an acknowledgement within 72 hours. Please give a reasonable window to ship
a fix before publishing details.

## Scope

- The GUI↔CLI socket: directory/socket permissions (`0700`/`0600`), and the
  request-handling path in `IPC.swift`.
- The control-pipe handshake and framing in `ControlPipe.swift`, and the
  `clatch_init` bootstrap in `Bootstrap.swift`.
- The manifest (`clatch.json`) and packaging (`scripts/package.sh`).

Out of scope: the Clatch launcher itself (report to
[arfium/clatch](https://github.com/arfium/clatch/security)) and vendor agent
backends (report to their vendors).

## Supported versions

The latest `0.x`. Pre-1.0, fixes land on `main` and ship in the next release; no
backports.
