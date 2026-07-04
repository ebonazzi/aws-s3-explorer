# CLAUDE.md — Personal Context for Claude Code AI Sessions


## Languages I Use Today

- **Java** Started using Java in 1997 spent most of my professional life around Java and its ecosystem (J2EE, Tommee, all majort Java libraries, JDBC, Servlets, JSP, etc.)
- **Rust** — I want Rust to become my primary language for all new development. I prefer Rust whenever possible, even if another language might seem simpler. Always suggest Rust-first solutions.
- **SQL** — I know relational theory deeply and write complex queries comfortably.
- **C** — comfortable but now stale. If I have to use or write some C, this is OK.
  I will need the OS platform specific compile, link, build instructions for C for all OS platforms.
  I am happy to use C libraries from Rust, Python, etc and will need detailed instructions.
- **C#** — basic knowledge. I do not want to write or manage any C# code.
  I will use C# libraries via Rust FFI or `cxx` crate when no Rust alternative exists.
  Provide the full build.rs + bindgen or cxx bridge setup — don't assume I know the C# side.
  I will need detailed instructions for C# related tasks.
- **Python** — you may use Python if I explicitly ask for it. If Python uses libraries written in C,
  C# or Rust then I will need detailed instructions for their installation and use.
  When using Python I prefer, but not require, using Rust-based `uv` Python tooling and
  `ruff` Python code formatter and linter.

  For Python code style:
    - Annotate all function parameters and return types explicitly.
    - Prefer `X | None` over `Optional[X]` (Python 3.10+ union syntax).
    - Avoid `Any` unless genuinely unavoidable.
    - Target Pylance `standard` type checking mode compatibility.

---

## Languages and Technologies I Do NOT Know

JavaScript, TypeScript, Go, gRPC, HTML, CSS, Mojo, Scala, Zig.
I understand all of these at a conceptual level but have no hands-on experience.
If a solution requires these, explain what the code does and why — don't assume I'll just know.

---

## Databases I DO Know

Oracle, PostgreSQL, Microsoft SQL Server,  Parquet Columnar Stores, Amazon Redshift, Apache Spark, Apache Iceberg

I am experienced in both **transactional (OLTP)** and **data warehousing and BI** systems,
including design, development, performance tuning, and troubleshooting.
I prefer but do not require Rust-native database options (Polars, Apache Arrow, DataFusion, etc).
They can be, preferably, local or cloud-based. I strongly prefer free-of-charge data products.

---

## Development Environment

- **Primary IDE editor:** All JetBrains IDE on  Linux Mint: Intellij, Pycharm, RustRover, Datagrip
- **Operating systems:** Linux Mint 22.3 laptop on a Lenovo Legion. Occasionally I use/configure/interact with Rocky Linux VMs I have on my Hetzner cloud provider or my HomeLab
- **Database:** PostgreSQL running on Linux Mint. PostgreSQL mostly on my systems, my customers also make me use and code against MS Sql Server
- **Rust toolchain:** nightly (run `rustup show` to confirm active version)

---

## How I Want Claude to Respond

### Code Solutions

- Always verify crate versions on crates.io before use; treat all suggested versions as approximate.
- Use the most recent Rust edition at the time of my asking (Rust 2024 or later, if it's stable),
  unless a lower edition is required for the project.
- For any Rust library/crate used, briefly explain what it does and why you chose it. I want to
  learn the ecosystem, not just get working code.
- If there are multiple valid approaches, briefly note the trade-offs before picking one.
- Always include `Cargo.toml` dependencies when providing Rust code.
- Always use standard `rustfmt` formatting for generated Rust code.
- Aim for zero `cargo clippy` warnings (prefer pedantic clippy).
- Aim for zero Rust compiler warnings in all generated code.
- Use Rust modules (mod) and libraries (lib.rs) as much as possible to build good readable
  maintainable code. The use of these features will be project specific.

### Git Conventions

- Conventional commit messages (feat:, fix:, chore:, docs:, refactor:)
- Supply relevant git commands to help me save my github projects once the task is done and tested.
- Always run `cargo fmt`, `cargo clippy`, `cargo test` before committing.
- Commit message body should reference the compiler/clippy clean state if a refactor.

### Claude Code Behaviour

- Think before coding. Always start with Planning Mode. If something is ambiguous, ask.
  Don't silently pick one interpretation and run with it. Surface tradeoffs, stop when confused.
- Before making changes to multiple files, show me the plan first and wait for approval.
- When fixing Rust compiler errors, run `cargo check`, `cargo clippy` (pedantic) after each fix
  to confirm it compiles.
- Prefer targeted edits over rewriting entire files unless a rewrite is clearly better.
- When suggesting new Rust crates, check if a similar capability already exists in the
  Project Conventions section before introducing a new dependency.

### Explanations

- When using technologies I am unfamiliar with (HTML, CSS, JS, TS, web servers, frameworks, etc.),
  explain the underlying concepts — web server model, request/response lifecycle, how a
  crate wraps a protocol, etc. Treat me as an expert learner, not a beginner.
- Go deep when depth is warranted — memory layout, OS internals, DB internals, protocol details, etc.
- I am keen to learn Rust idiomatic coding practices and common Rust design patterns.

---

## Rust-Specific Preferences

### Error Handling Philosophy

- Use `?` operator consistently — avoid `.unwrap()` and `.expect()` in production code.
- `.expect()` is acceptable in `#[cfg(test)]` blocks and `examples/` only.
- In `src/` production code, replace with `?` or explain in a comment why the `Err` case is unreachable.
- Never use `.unwrap()` without a comment explaining why it is safe.

### Unsafe Code

- Avoid `unsafe` blocks unless necessary.
- If `unsafe` is required, always add a `// SAFETY:` comment explaining the invariants.

### Dependencies Philosophy

- Prefer well-maintained crates with recent commits and high download counts.
- Avoid pulling in heavy dependencies for trivial tasks (e.g. don't add a crate just for one utility function).
- Add `#![forbid(unsafe_code)]` to library crates unless FFI or performance requires otherwise.
- Do use Rust FFI if it is necessary or strongly preferable over pure Rust alternatives.
- Create and use Rust macros if it will aid the solution for readability and maintainability.

---

## Project Conventions (override per project as needed)

- Error handling: prefer `anyhow` for application code, `thiserror` for library code.
- Async runtime: prefer `tokio` and the `tokio` software stack unless there is a very good reason
  not to. The use of tokio features will depend on the project.
- Logging: prefer `tracing` crate.
- CLI: prefer `clap` with derive macros.
- Serialisation: prefer `serde` with `serde_json`.
- Testing: include unit tests by default using standard `#[test]`;
  use `cargo-nextest` if test organisation is complex.
- Do use common Rust design patterns (such as builder pattern, typestate, traits, newtype, etc).
- For CPU concurrency consider `rayon` if appropriate but offer better alternatives if they exist.
- I strongly prefer an Allman-style coding convention. I want curly brackets on the next line, never on the same line. That forces me to use nighly rust builds. I want to keep it that way.


---

## What I Am Currently Learning / Interested In

- Rust ecosystem (crates, patterns, modules, libraries, idioms, best practices, performance)
- Using Claude AI models and Claude Code effectively as a software development tool
- Rust + database integration (especially PostgreSQL via `sqlx` or `tokio-postgres` or others)
- Web-facing Rust (even though I don't know the web stack — I want to learn it, via Rust)
- Rich GUI desktop native apps in Rust (such as egui crate/framework or others)
- WebAssembly (WASM) and Rust
- Using GPU in addition to CPU from Rust where appropriate (e.g. via `wgpu` or `cudarc` crates)

---

*Last updated: 2026-07-02*
