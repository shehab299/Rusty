# Code style

## Error naming

- Use application-owned error types in application code; wrap dependency errors at integration boundaries.
- Prefer concise variants named after subsystems or broad error categories.
- Put specific failure details in the error message rather than introducing a variant for every failure case.
- Add a dedicated variant only when callers need to handle that failure differently.
- Avoid operation-specific names when a broader category is sufficient.
- Preserve underlying errors as sources when wrapping them.

## General

- Prefer idiomatic Rust conversions (From/Into) for straightforward type transformations.
- Prefer direct access over trivial getters
- Use composition instead of duplicating fields.
- Choose names that describe actual behavior. Reading records isn’t replaying state; marking interruptions isn’t stopping tools.

## Module ownership

- Place domain types and their behavior in the subsystem that owns their meaning, not in modules that merely transport or display them.
- Keep communication modules focused on commands, events, and protocol payloads; import domain types from their owning modules.
