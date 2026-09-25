//! Home of the JobHunt MCP (Model Context Protocol) server.
//!
//! This crate intentionally contains no functionality yet. It exists so the
//! workspace has the boundary the architecture calls for: the MCP server is a
//! front-end, a sibling of `jobhunt-cli`, and like the CLI it will depend on
//! `jobhunt-jobs` (domain + discovery pipeline), `jobhunt-sources` (adapters),
//! `jobhunt-profile` (the career profile, through `ProfileService`),
//! `jobhunt-resume` (resume reading) and `jobhunt-storage` (persistence)
//! rather than reimplementing any of them.
//!
//! When the server is built, the configuration and wiring that currently live
//! in `jobhunt-cli` (config loading, opening the store, building sources)
//! should move into a small shared crate that both front-ends use.
