---
name: Project Architecture Notes
description: Key architectural decisions for the main project
type: architecture
---
# Project Architecture

This project uses a microservices pattern with event-driven communication.

The data pipeline flows through:
1. [[Ingestion Service]] - processes incoming data
2. [[Transform Pipeline]] - normalizes and enriches
3. [[Storage Layer]] - persists to SQLite

## Key Decisions
- Use CGO-free SQLite driver for cross-platform builds
- WAL mode for concurrent read/write
- [[Security Guard]] handles PII redaction at the boundary

Related: [[Deployment Guide]]
